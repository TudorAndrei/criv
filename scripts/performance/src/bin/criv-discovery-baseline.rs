use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};

const RUN_SCHEMA: &str = "criv.discovery-run.v1";
const SAMPLE_SCHEMA: &str = "criv.discovery-sample.v1";
const SUMMARY_SCHEMA: &str = "criv.discovery-summary.v1";
const PROBE_SCHEMA: &str = "criv.discovery-probe.v1";
const PROBE_PREFIX: &str = "criv-discovery-probe-v1 ";
const ROOT_ENV: &str = "CRIV_DISCOVERY_PROBE_ROOT";

#[derive(Debug, Parser)]
#[command(
    name = "criv-discovery-baseline",
    about = "Measure criv discovery profiles through the test-only selector probe"
)]
struct Args {
    /// Repository that contains the criv source and probe hook.
    #[arg(long, default_value = ".")]
    repository_root: PathBuf,
    /// Repository tree whose file discovery is measured.
    #[arg(long)]
    workload_root: PathBuf,
    /// Full local workload inventory. Required for evidence runs.
    #[arg(long)]
    workload_inventory: Option<PathBuf>,
    /// Stable workload name for a smoke run without an inventory.
    #[arg(long)]
    workload_id: Option<String>,
    /// BLAKE3 workload identity for a smoke run without an inventory.
    #[arg(long)]
    workload_digest: Option<String>,
    /// Compiled criv library-test executable. Build it automatically when omitted.
    #[arg(long)]
    probe_executable: Option<PathBuf>,
    /// Receipt for a test-only adapter applied to an immutable production revision.
    #[arg(long)]
    adapter_receipt: Option<PathBuf>,
    /// Human-readable identity for the probe source and adapter.
    #[arg(long, default_value = "current-main-control")]
    probe_label: String,
    /// Discovery profile to measure. Repeat to select more than one.
    #[arg(long = "profile", value_enum)]
    profiles: Vec<Profile>,
    /// Number of recorded samples per profile and attempt.
    #[arg(long, default_value_t = 5)]
    samples: usize,
    /// Permit fewer than five samples for harness smoke tests only.
    #[arg(long)]
    allow_low_samples: bool,
    /// Include full selected path lists for a one-sample correctness run.
    #[arg(long)]
    dump_paths: bool,
    /// Parent directory for a new unique result directory.
    #[arg(long, default_value = "target/discovery-results")]
    results_root: PathBuf,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
enum Profile {
    Source,
    SourceCandidates,
    Vault,
    Markdown,
}

impl Profile {
    fn id(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::SourceCandidates => "source_candidates",
            Self::Vault => "vault",
            Self::Markdown => "markdown",
        }
    }

    fn test_name(self) -> String {
        format!("discovery_probe::{}", self.id())
    }
}

const ALL_PROFILES: [Profile; 4] = [
    Profile::Source,
    Profile::SourceCandidates,
    Profile::Vault,
    Profile::Markdown,
];

#[derive(Debug, Serialize)]
struct RunIdentity {
    schema: &'static str,
    run_id: String,
    started_at_utc: String,
    repository_root: String,
    revision: String,
    dirty: bool,
    workload_root: String,
    workload_id: String,
    workload_digest: String,
    probe_label: String,
    probe_executable: String,
    probe_digest: String,
    harness: String,
    harness_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    adapter: Option<AdapterIdentity>,
    rustc_verbose: String,
    operating_system: String,
    architecture: String,
    processor: String,
    machine_digest: String,
    samples: usize,
    paths_dumped: bool,
    profiles: Vec<&'static str>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AdapterIdentity {
    schema: String,
    revision: String,
    commit: String,
    patch_digest: String,
    probe_digest: String,
    output: String,
    #[serde(default)]
    receipt_digest: String,
}

#[derive(Debug, Deserialize)]
struct WorkloadInventoryHeader {
    schema: String,
    workload_id: String,
    workload_digest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProbeGroup {
    name: String,
    selected_files: usize,
    selected_bytes: u64,
    path_digest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProbeOutput {
    schema: String,
    profile: String,
    selected_files: usize,
    selected_bytes: u64,
    path_digest: String,
    groups: Vec<ProbeGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    paths: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Serialize)]
struct SampleRow {
    schema: &'static str,
    run_id: String,
    workload_id: String,
    workload_digest: String,
    probe_label: String,
    probe_digest: String,
    profile: String,
    attempt: usize,
    sample: usize,
    exit_status: i32,
    real_seconds: f64,
    user_seconds: Option<f64>,
    system_seconds: Option<f64>,
    peak_rss_bytes: Option<u64>,
    stdout_digest: String,
    stderr_digest: String,
    stdout_path: String,
    stderr_path: String,
    probe: Option<ProbeOutput>,
}

#[derive(Debug, Clone, Serialize)]
struct MetricSummary {
    minimum: f64,
    median: f64,
    maximum: f64,
    median_absolute_deviation: f64,
}

#[derive(Debug, Serialize)]
struct AttemptSummary {
    profile: String,
    attempt: usize,
    successful_samples: usize,
    failed_samples: usize,
    real_seconds: Option<MetricSummary>,
    user_seconds: Option<MetricSummary>,
    system_seconds: Option<MetricSummary>,
    peak_rss_bytes: Option<MetricSummary>,
    relative_mad: Option<f64>,
    unstable: bool,
    selected_files: Option<usize>,
    selected_bytes: Option<u64>,
    path_digest: Option<String>,
}

#[derive(Debug, Serialize)]
struct SummaryDocument<'a> {
    schema: &'static str,
    run: &'a RunIdentity,
    attempts: Vec<AttemptSummary>,
}

struct ChildOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    user_seconds: Option<f64>,
    system_seconds: Option<f64>,
    peak_rss_bytes: Option<u64>,
}

fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("criv-discovery-baseline: {error}");
        std::process::exit(1);
    }
}

fn run(mut args: Args) -> Result<(), String> {
    validate_args(&args)?;
    let repository_root = canonical(&args.repository_root)?;
    let workload_root = canonical(&args.workload_root)?;
    let adapter = args
        .adapter_receipt
        .as_ref()
        .map(|path| read_adapter_identity(path, &repository_root))
        .transpose()?;
    let (workload_id, workload_digest) = workload_identity(&args)?;
    let probe_executable = match args.probe_executable.take() {
        Some(path) => canonical(&path)?,
        None => build_probe(&repository_root)?,
    };
    validate_executable(&probe_executable)?;

    let profiles = if args.profiles.is_empty() {
        ALL_PROFILES.to_vec()
    } else {
        args.profiles.sort();
        args.profiles.dedup();
        args.profiles
    };
    let probe_digest = file_digest(&probe_executable)?;
    let harness = std::env::current_exe().map_err(display_error)?;
    let harness_digest = file_digest(&harness)?;
    let (revision, dirty) = match adapter.as_ref() {
        Some(adapter) => (adapter.commit.clone(), true),
        None => (
            command_text(&repository_root, "git", &["rev-parse", "HEAD"])
                .unwrap_or_else(|_| "unavailable".into()),
            command_text(&repository_root, "git", &["status", "--porcelain"])
                .map(|value| !value.trim().is_empty())
                .unwrap_or(true),
        ),
    };
    let started_at_utc = command_text(&repository_root, "date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .unwrap_or_else(|_| unix_millis().to_string());
    let rustc_verbose = command_text(&repository_root, "rustc", &["--version", "--verbose"])
        .unwrap_or_else(|_| "unavailable".into());
    let operating_system = command_text(&repository_root, "uname", &["-sr"])
        .unwrap_or_else(|_| std::env::consts::OS.into());
    let architecture = std::env::consts::ARCH.to_string();
    let processor = processor_identity(&repository_root);
    let machine_digest = bytes_digest(
        format!("{operating_system}\0{architecture}\0{processor}\0{rustc_verbose}").as_bytes(),
    );
    let run_seed = format!(
        "{}\0{}\0{}\0{}\0{}",
        revision,
        workload_digest,
        probe_digest,
        started_at_utc,
        std::process::id()
    );
    let run_id = blake3::hash(run_seed.as_bytes()).to_hex()[..16].to_string();
    let run_identity = RunIdentity {
        schema: RUN_SCHEMA,
        run_id: run_id.clone(),
        started_at_utc,
        repository_root: repository_root.display().to_string(),
        revision,
        dirty,
        workload_root: workload_root.display().to_string(),
        workload_id,
        workload_digest,
        probe_label: args.probe_label.clone(),
        probe_executable: probe_executable.display().to_string(),
        probe_digest: probe_digest.clone(),
        harness: harness.display().to_string(),
        harness_digest,
        adapter,
        rustc_verbose,
        operating_system,
        architecture,
        processor,
        machine_digest,
        samples: args.samples,
        paths_dumped: args.dump_paths,
        profiles: profiles.iter().map(|profile| profile.id()).collect(),
    };

    let result_dir = create_result_dir(&args.results_root, &run_id)?;
    fs::create_dir(result_dir.join("outputs")).map_err(display_error)?;
    write_json(result_dir.join("run.json"), &run_identity)?;
    let mut raw =
        BufWriter::new(File::create(result_dir.join("samples.jsonl")).map_err(display_error)?);
    let mut rows = Vec::new();
    for profile in profiles {
        let first = run_attempt(
            &probe_executable,
            &workload_root,
            &result_dir,
            &run_identity,
            profile,
            1,
            args.samples,
            args.dump_paths,
            &mut raw,
        )?;
        let unstable = attempt_summary(profile, 1, &first).unstable;
        rows.extend(first);
        if unstable {
            let second = run_attempt(
                &probe_executable,
                &workload_root,
                &result_dir,
                &run_identity,
                profile,
                2,
                args.samples,
                args.dump_paths,
                &mut raw,
            )?;
            rows.extend(second);
        }
    }
    raw.flush().map_err(display_error)?;

    let mut summaries = Vec::new();
    for profile in &run_identity.profiles {
        for attempt in 1..=2 {
            let selected = rows
                .iter()
                .filter(|row| row.profile == *profile && row.attempt == attempt)
                .collect::<Vec<_>>();
            if selected.is_empty() {
                continue;
            }
            let profile = Profile::from_id(profile)?;
            summaries.push(attempt_summary_refs(profile, attempt, &selected));
        }
    }
    let summary = SummaryDocument {
        schema: SUMMARY_SCHEMA,
        run: &run_identity,
        attempts: summaries,
    };
    write_json(result_dir.join("summary.json"), &summary)?;

    println!("{}", result_dir.display());
    let failed = rows
        .iter()
        .any(|row| row.exit_status != 0 || row.probe.is_none());
    let unstable_identity = ensure_stable_identity(&rows).err();
    match (failed, unstable_identity) {
        (true, Some(error)) => {
            return Err(format!(
                "one or more discovery samples failed and output identity was unstable ({error}); raw evidence was preserved"
            ));
        }
        (true, None) => {
            return Err("one or more discovery samples failed; raw evidence was preserved".into());
        }
        (false, Some(error)) => return Err(error),
        (false, None) => {}
    }
    Ok(())
}

fn processor_identity(root: &Path) -> String {
    command_text(root, "sysctl", &["-n", "machdep.cpu.brand_string"])
        .or_else(|_| {
            fs::read_to_string("/proc/cpuinfo")
                .map_err(display_error)?
                .lines()
                .find_map(|line| line.strip_prefix("model name\t: "))
                .map(str::to_string)
                .ok_or_else(|| "processor model unavailable".to_string())
        })
        .or_else(|_| std::env::var("PROCESSOR_IDENTIFIER").map_err(display_error))
        .unwrap_or_else(|_| "unknown".into())
}

fn read_adapter_identity(path: &Path, repository_root: &Path) -> Result<AdapterIdentity, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read adapter receipt {}: {error}", path.display()))?;
    let mut identity: AdapterIdentity = serde_json::from_slice(&bytes).map_err(display_error)?;
    if identity.schema != "criv.discovery-adapter.v1" {
        return Err(format!(
            "unsupported discovery adapter schema {}",
            identity.schema
        ));
    }
    let output = canonical(Path::new(&identity.output))?;
    if output != repository_root {
        return Err(format!(
            "adapter receipt output {} does not match repository-root {}",
            output.display(),
            repository_root.display()
        ));
    }
    identity.receipt_digest = bytes_digest(&bytes);
    Ok(identity)
}

impl Profile {
    fn from_id(value: &str) -> Result<Self, String> {
        match value {
            "source" => Ok(Self::Source),
            "source_candidates" => Ok(Self::SourceCandidates),
            "vault" => Ok(Self::Vault),
            "markdown" => Ok(Self::Markdown),
            _ => Err(format!("unknown discovery profile {value}")),
        }
    }
}

fn validate_args(args: &Args) -> Result<(), String> {
    if args.probe_label.trim().is_empty() {
        return Err("probe-label must not be empty".into());
    }
    if args.samples == 0 || (args.samples < 5 && !args.allow_low_samples) {
        return Err(
            "samples must be at least 5; use --allow-low-samples only for smoke tests".into(),
        );
    }
    if args.dump_paths && (!args.allow_low_samples || args.samples != 1) {
        return Err(
            "dump-paths requires a one-sample run with --allow-low-samples because path output is not timing evidence"
                .into(),
        );
    }
    match (&args.workload_inventory, args.allow_low_samples) {
        (Some(_), _) => {
            if args.workload_id.is_some() || args.workload_digest.is_some() {
                return Err(
                    "workload-id and workload-digest must be omitted when workload-inventory is used"
                        .into(),
                );
            }
        }
        (None, true) => {
            validate_explicit_workload_identity(
                args.workload_id.as_deref(),
                args.workload_digest.as_deref(),
            )?;
        }
        (None, false) => {
            return Err("evidence runs require --workload-inventory".into());
        }
    }
    Ok(())
}

fn workload_identity(args: &Args) -> Result<(String, String), String> {
    if let Some(path) = &args.workload_inventory {
        let input = File::open(path)
            .map_err(|error| format!("failed to open inventory {}: {error}", path.display()))?;
        let inventory: WorkloadInventoryHeader =
            serde_json::from_reader(BufReader::new(input)).map_err(display_error)?;
        if inventory.schema != "criv.discovery-inventory.v1" {
            return Err(format!(
                "unsupported workload inventory schema {}",
                inventory.schema
            ));
        }
        validate_explicit_workload_identity(
            Some(&inventory.workload_id),
            Some(&inventory.workload_digest),
        )?;
        Ok((inventory.workload_id, inventory.workload_digest))
    } else {
        validate_explicit_workload_identity(
            args.workload_id.as_deref(),
            args.workload_digest.as_deref(),
        )
    }
}

fn validate_explicit_workload_identity(
    workload_id: Option<&str>,
    workload_digest: Option<&str>,
) -> Result<(String, String), String> {
    let workload_id = workload_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "workload-id must not be empty".to_string())?;
    let workload_digest = workload_digest.ok_or_else(|| {
        "workload-digest must be a 64-character hexadecimal BLAKE3 digest".to_string()
    })?;
    if workload_digest.len() != 64 || !workload_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("workload-digest must be a 64-character hexadecimal BLAKE3 digest".into());
    }
    Ok((workload_id.into(), workload_digest.to_ascii_lowercase()))
}

fn build_probe(repository_root: &Path) -> Result<PathBuf, String> {
    let output = Command::new("cargo")
        .args([
            "test",
            "--locked",
            "--release",
            "--lib",
            "--no-run",
            "--message-format=json-render-diagnostics",
        ])
        .current_dir(repository_root)
        .output()
        .map_err(display_error)?;
    if !output.status.success() {
        return Err(format!(
            "failed to build discovery probe:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let mut executable = None;
    for line in BufReader::new(output.stdout.as_slice()).lines() {
        let line = line.map_err(display_error)?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let is_criv_test = value.get("reason").and_then(|value| value.as_str())
            == Some("compiler-artifact")
            && value
                .pointer("/target/name")
                .and_then(|value| value.as_str())
                == Some("criv")
            && value
                .pointer("/profile/test")
                .and_then(|value| value.as_bool())
                == Some(true);
        if is_criv_test {
            executable = value
                .get("executable")
                .and_then(|value| value.as_str())
                .map(PathBuf::from);
        }
    }
    executable.ok_or_else(|| "cargo did not report the criv library-test executable".into())
}

#[allow(clippy::too_many_arguments)]
fn run_attempt(
    executable: &Path,
    workload_root: &Path,
    result_dir: &Path,
    run: &RunIdentity,
    profile: Profile,
    attempt: usize,
    samples: usize,
    dump_paths: bool,
    raw: &mut BufWriter<File>,
) -> Result<Vec<SampleRow>, String> {
    run_probe(executable, workload_root, profile, dump_paths)?;
    let mut rows = Vec::with_capacity(samples);
    for sample in 1..=samples {
        let start = Instant::now();
        let output = run_probe(executable, workload_root, profile, dump_paths)?;
        let real_seconds = start.elapsed().as_secs_f64();
        let prefix = format!("{}-attempt-{attempt}-sample-{sample:03}", profile.id());
        let stdout_path = Path::new("outputs").join(format!("{prefix}.stdout"));
        let stderr_path = Path::new("outputs").join(format!("{prefix}.stderr"));
        fs::write(result_dir.join(&stdout_path), &output.stdout).map_err(display_error)?;
        fs::write(result_dir.join(&stderr_path), &output.stderr).map_err(display_error)?;
        let probe = parse_probe(&output.stdout, profile).ok();
        let row = SampleRow {
            schema: SAMPLE_SCHEMA,
            run_id: run.run_id.clone(),
            workload_id: run.workload_id.clone(),
            workload_digest: run.workload_digest.clone(),
            probe_label: run.probe_label.clone(),
            probe_digest: run.probe_digest.clone(),
            profile: profile.id().into(),
            attempt,
            sample,
            exit_status: output.status.code().unwrap_or(-1),
            real_seconds,
            user_seconds: output.user_seconds,
            system_seconds: output.system_seconds,
            peak_rss_bytes: output.peak_rss_bytes,
            stdout_digest: bytes_digest(&output.stdout),
            stderr_digest: bytes_digest(&output.stderr),
            stdout_path: stdout_path.display().to_string(),
            stderr_path: stderr_path.display().to_string(),
            probe,
        };
        serde_json::to_writer(&mut *raw, &row).map_err(display_error)?;
        raw.write_all(b"\n").map_err(display_error)?;
        rows.push(row);
    }
    Ok(rows)
}

fn run_probe(
    executable: &Path,
    workload_root: &Path,
    profile: Profile,
    dump_paths: bool,
) -> Result<ChildOutput, String> {
    let mut command = Command::new(executable);
    command
        .args([
            "--ignored",
            "--exact",
            &profile.test_name(),
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROOT_ENV, workload_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if dump_paths {
        command.env("CRIV_DISCOVERY_PROBE_DUMP", "1");
    }
    run_child(command)
}

fn parse_probe(stdout: &[u8], profile: Profile) -> Result<ProbeOutput, String> {
    let text = String::from_utf8_lossy(stdout);
    let payload = text
        .lines()
        .find_map(|line| {
            line.find(PROBE_PREFIX)
                .map(|index| &line[index + PROBE_PREFIX.len()..])
        })
        .ok_or_else(|| "probe output marker is missing".to_string())?;
    let output: ProbeOutput = serde_json::from_str(payload).map_err(display_error)?;
    if output.schema != PROBE_SCHEMA {
        return Err(format!("unsupported probe schema {}", output.schema));
    }
    if output.profile != profile.id() {
        return Err(format!(
            "probe returned profile {} for {}",
            output.profile,
            profile.id()
        ));
    }
    Ok(output)
}

fn attempt_summary(profile: Profile, attempt: usize, rows: &[SampleRow]) -> AttemptSummary {
    let rows = rows.iter().collect::<Vec<_>>();
    attempt_summary_refs(profile, attempt, &rows)
}

fn attempt_summary_refs(profile: Profile, attempt: usize, rows: &[&SampleRow]) -> AttemptSummary {
    let successful = rows
        .iter()
        .filter(|row| row.exit_status == 0 && row.probe.is_some())
        .copied()
        .collect::<Vec<_>>();
    let real_seconds = metric(successful.iter().map(|row| row.real_seconds).collect());
    let relative_mad = real_seconds.as_ref().and_then(|summary| {
        (summary.median > 0.0).then_some(summary.median_absolute_deviation / summary.median)
    });
    let first = successful.first().and_then(|row| row.probe.as_ref());
    AttemptSummary {
        profile: profile.id().into(),
        attempt,
        successful_samples: successful.len(),
        failed_samples: rows.len() - successful.len(),
        real_seconds,
        user_seconds: metric(
            successful
                .iter()
                .filter_map(|row| row.user_seconds)
                .collect(),
        ),
        system_seconds: metric(
            successful
                .iter()
                .filter_map(|row| row.system_seconds)
                .collect(),
        ),
        peak_rss_bytes: metric(
            successful
                .iter()
                .filter_map(|row| row.peak_rss_bytes.map(|value| value as f64))
                .collect(),
        ),
        relative_mad,
        unstable: relative_mad.is_some_and(|value| value > 0.10),
        selected_files: first.map(|probe| probe.selected_files),
        selected_bytes: first.map(|probe| probe.selected_bytes),
        path_digest: first.map(|probe| probe.path_digest.clone()),
    }
}

fn ensure_stable_identity(rows: &[SampleRow]) -> Result<(), String> {
    for profile in ALL_PROFILES {
        let identities = rows
            .iter()
            .filter(|row| row.profile == profile.id() && row.exit_status == 0)
            .filter_map(|row| row.probe.as_ref())
            .map(|probe| {
                (
                    probe.path_digest.as_str(),
                    probe.selected_files,
                    probe.selected_bytes,
                )
            })
            .collect::<BTreeSet<_>>();
        if identities.len() > 1 {
            return Err(format!(
                "{} discovery output identity changed between samples",
                profile.id()
            ));
        }
    }
    Ok(())
}

fn metric(mut values: Vec<f64>) -> Option<MetricSummary> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let median_value = median(&values);
    let mut deviations = values
        .iter()
        .map(|value| (value - median_value).abs())
        .collect::<Vec<_>>();
    deviations.sort_by(f64::total_cmp);
    Some(MetricSummary {
        minimum: values[0],
        median: median_value,
        maximum: *values.last().unwrap(),
        median_absolute_deviation: median(&deviations),
    })
}

fn median(values: &[f64]) -> f64 {
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

#[cfg(unix)]
fn run_child(mut command: Command) -> Result<ChildOutput, String> {
    use std::os::unix::process::ExitStatusExt;

    let mut child = command.spawn().map_err(display_error)?;
    let pid = child.id() as libc::pid_t;
    let mut status = 0;
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    loop {
        // SAFETY: pid is the child process, status and usage are valid output pointers.
        let waited = unsafe { libc::wait4(pid, &mut status, 0, usage.as_mut_ptr()) };
        if waited == pid {
            break;
        }
        if waited == -1 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
        {
            continue;
        }
        return Err(format!(
            "wait4 failed for child {pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: wait4 returned the child pid and initialized usage.
    let usage = unsafe { usage.assume_init() };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(|| "child stdout pipe is missing".to_string())?
        .read_to_end(&mut stdout)
        .map_err(display_error)?;
    child
        .stderr
        .take()
        .ok_or_else(|| "child stderr pipe is missing".to_string())?
        .read_to_end(&mut stderr)
        .map_err(display_error)?;
    Ok(ChildOutput {
        status: ExitStatus::from_raw(status),
        stdout,
        stderr,
        user_seconds: Some(timeval_seconds(usage.ru_utime)),
        system_seconds: Some(timeval_seconds(usage.ru_stime)),
        peak_rss_bytes: Some(peak_rss_bytes(&usage)),
    })
}

#[cfg(unix)]
fn timeval_seconds(value: libc::timeval) -> f64 {
    value.tv_sec as f64 + value.tv_usec as f64 / 1_000_000.0
}

#[cfg(all(unix, target_os = "macos"))]
fn peak_rss_bytes(usage: &libc::rusage) -> u64 {
    usage.ru_maxrss.max(0) as u64
}

#[cfg(all(unix, not(target_os = "macos")))]
fn peak_rss_bytes(usage: &libc::rusage) -> u64 {
    (usage.ru_maxrss.max(0) as u64).saturating_mul(1024)
}

#[cfg(windows)]
fn run_child(mut command: Command) -> Result<ChildOutput, String> {
    use std::mem::{MaybeUninit, size_of};
    use std::os::windows::io::AsRawHandle;
    use std::thread;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetProcessTimes;

    let mut child = command.spawn().map_err(display_error)?;
    let handle = child.as_raw_handle() as *mut core::ffi::c_void;
    let mut observed_peak = 0_u64;
    let status = loop {
        let mut counters = MaybeUninit::<PROCESS_MEMORY_COUNTERS>::zeroed();
        // SAFETY: handle belongs to the live child and counters has the documented size.
        let ok = unsafe {
            GetProcessMemoryInfo(
                handle,
                counters.as_mut_ptr(),
                size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
        };
        if ok != 0 {
            // SAFETY: the successful call initialized counters.
            observed_peak =
                observed_peak.max(unsafe { counters.assume_init() }.PeakWorkingSetSize as u64);
        }
        if let Some(status) = child.try_wait().map_err(display_error)? {
            break status;
        }
        thread::sleep(Duration::from_millis(2));
    };
    let mut creation = MaybeUninit::<FILETIME>::uninit();
    let mut exit = MaybeUninit::<FILETIME>::uninit();
    let mut kernel = MaybeUninit::<FILETIME>::uninit();
    let mut user = MaybeUninit::<FILETIME>::uninit();
    // SAFETY: the child handle stays valid until Child is dropped and all pointers are valid.
    let has_times = unsafe {
        GetProcessTimes(
            handle,
            creation.as_mut_ptr(),
            exit.as_mut_ptr(),
            kernel.as_mut_ptr(),
            user.as_mut_ptr(),
        )
    } != 0;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(|| "child stdout pipe is missing".to_string())?
        .read_to_end(&mut stdout)
        .map_err(display_error)?;
    child
        .stderr
        .take()
        .ok_or_else(|| "child stderr pipe is missing".to_string())?
        .read_to_end(&mut stderr)
        .map_err(display_error)?;
    let (user_seconds, system_seconds) = if has_times {
        // SAFETY: GetProcessTimes initialized kernel and user on success.
        let kernel = unsafe { kernel.assume_init() };
        // SAFETY: GetProcessTimes initialized kernel and user on success.
        let user = unsafe { user.assume_init() };
        (Some(filetime_seconds(user)), Some(filetime_seconds(kernel)))
    } else {
        (None, None)
    };
    Ok(ChildOutput {
        status,
        stdout,
        stderr,
        user_seconds,
        system_seconds,
        peak_rss_bytes: (observed_peak > 0).then_some(observed_peak),
    })
}

#[cfg(windows)]
fn filetime_seconds(value: windows_sys::Win32::Foundation::FILETIME) -> f64 {
    let ticks = ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64;
    ticks as f64 / 10_000_000.0
}

#[cfg(not(any(unix, windows)))]
fn run_child(mut command: Command) -> Result<ChildOutput, String> {
    let output = command.output().map_err(display_error)?;
    Ok(ChildOutput {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
        user_seconds: None,
        system_seconds: None,
        peak_rss_bytes: None,
    })
}

fn canonical(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|error| format!("failed to resolve {}: {error}", path.display()))
}

fn validate_executable(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!(
            "probe executable is not a file: {}",
            path.display()
        ));
    }
    Ok(())
}

fn create_result_dir(root: &Path, run_id: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(display_error)?;
    for suffix in 0_u32.. {
        let name = if suffix == 0 {
            run_id.to_string()
        } else {
            format!("{run_id}-{suffix}")
        };
        let path = root.join(name);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    unreachable!()
}

fn command_text(root: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(display_error)?;
    if !output.status.success() {
        return Err(format!("{program} {} failed", args.join(" ")));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(display_error)
}

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<(), String> {
    let mut output = BufWriter::new(File::create(path).map_err(display_error)?);
    serde_json::to_writer_pretty(&mut output, value).map_err(display_error)?;
    output.write_all(b"\n").map_err(display_error)
}

fn file_digest(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|bytes| bytes_digest(&bytes))
        .map_err(display_error)
}

fn bytes_digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> Args {
        Args {
            repository_root: ".".into(),
            workload_root: ".".into(),
            workload_inventory: None,
            workload_id: Some("fixture".into()),
            workload_digest: Some("a".repeat(64)),
            probe_executable: None,
            adapter_receipt: None,
            probe_label: "test".into(),
            profiles: vec![],
            samples: 5,
            allow_low_samples: false,
            dump_paths: false,
            results_root: "target/test-results".into(),
        }
    }

    #[test]
    fn evidence_requires_five_samples() {
        let mut args = args();
        args.samples = 4;
        assert_eq!(
            validate_args(&args).unwrap_err(),
            "samples must be at least 5; use --allow-low-samples only for smoke tests"
        );
    }

    #[test]
    fn smoke_runs_can_use_one_sample() {
        let mut args = args();
        args.samples = 1;
        args.allow_low_samples = true;
        assert!(validate_args(&args).is_ok());
    }

    #[test]
    fn path_dumps_are_correctness_runs_only() {
        let mut args = args();
        args.dump_paths = true;
        assert!(
            validate_args(&args)
                .unwrap_err()
                .contains("dump-paths requires a one-sample run")
        );
        args.samples = 1;
        args.allow_low_samples = true;
        assert!(validate_args(&args).is_ok());
    }

    #[test]
    fn workload_identity_requires_a_blake3_digest() {
        let mut args = args();
        args.allow_low_samples = true;
        args.workload_digest = Some("not-a-digest".into());
        assert_eq!(
            validate_args(&args).unwrap_err(),
            "workload-digest must be a 64-character hexadecimal BLAKE3 digest"
        );
    }

    #[test]
    fn evidence_requires_a_full_inventory() {
        let mut args = args();
        args.allow_low_samples = false;
        assert_eq!(
            validate_args(&args).unwrap_err(),
            "evidence runs require --workload-inventory"
        );
    }

    #[test]
    fn probe_parser_finds_output_inside_libtest_line() {
        let stdout = br#"running 1 test
test discovery_probe::source ... criv-discovery-probe-v1 {"schema":"criv.discovery-probe.v1","profile":"source","selected_files":1,"selected_bytes":8,"path_digest":"abc","groups":[]}
ok
"#;
        let output = parse_probe(stdout, Profile::Source).unwrap();
        assert_eq!(output.profile, "source");
        assert_eq!(output.selected_files, 1);
    }

    #[test]
    fn relative_mad_over_ten_percent_is_unstable() {
        let rows = [0.8, 0.9, 1.0, 1.2, 1.4]
            .into_iter()
            .enumerate()
            .map(|(index, real_seconds)| SampleRow {
                schema: SAMPLE_SCHEMA,
                run_id: "run".into(),
                workload_id: "fixture".into(),
                workload_digest: "a".repeat(64),
                probe_label: "test".into(),
                probe_digest: "b".repeat(64),
                profile: "source".into(),
                attempt: 1,
                sample: index + 1,
                exit_status: 0,
                real_seconds,
                user_seconds: None,
                system_seconds: None,
                peak_rss_bytes: None,
                stdout_digest: String::new(),
                stderr_digest: String::new(),
                stdout_path: String::new(),
                stderr_path: String::new(),
                probe: Some(ProbeOutput {
                    schema: PROBE_SCHEMA.into(),
                    profile: "source".into(),
                    selected_files: 1,
                    selected_bytes: 1,
                    path_digest: "identity".into(),
                    groups: vec![],
                    paths: None,
                }),
            })
            .collect::<Vec<_>>();
        assert!(attempt_summary(Profile::Source, 1, &rows).unstable);
    }
}
