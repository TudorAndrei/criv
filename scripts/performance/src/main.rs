mod generate;
mod manifest;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, ValueEnum};
use generate::{GeneratedWorkload, append_source_revision, generate, mutate_sources};
use manifest::LoadedManifest;
use serde::Serialize;
use tempfile::TempDir;

const RUN_SCHEMA: &str = "criv.performance-run.v2";
const SAMPLE_SCHEMA: &str = "criv.performance-sample.v2";
const SUMMARY_SCHEMA: &str = "criv.performance-summary.v2";

#[derive(Debug, Parser)]
#[command(
    name = "criv-perf",
    about = "Generate isolated criv workloads and preserve repeatable performance evidence"
)]
struct Args {
    /// Explicit criv executable to measure.
    #[arg(long, required = true)]
    binary: PathBuf,
    /// Cargo profile identity for the supplied binary.
    #[arg(long, required = true)]
    profile: String,
    /// Workload manifest; repeat to measure more than one.
    #[arg(long)]
    manifest: Vec<PathBuf>,
    /// Number of recorded samples per workload and case.
    #[arg(long, default_value_t = 5)]
    samples: usize,
    /// Permit fewer than three samples for harness smoke tests only.
    #[arg(long)]
    allow_low_samples: bool,
    /// Permit an explicit profile other than release for harness smoke tests only.
    #[arg(long)]
    allow_non_release: bool,
    /// Restrict the run to selected command cases; repeat as needed.
    #[arg(long = "case", value_enum)]
    cases: Vec<Case>,
    /// Parent directory in which a new unique result directory is created.
    #[arg(long, default_value = "target/performance-results")]
    results_root: PathBuf,
    /// Repository whose revision, manifests, and Rust metadata identify the run.
    #[arg(long, default_value = ".")]
    repository_root: PathBuf,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, ValueEnum)]
enum Case {
    WatchOnceCold,
    WatchOnceWarm,
    WatchOnceChanged,
    WatchOnceSemanticChanged,
    Check,
    EnforceCi,
    QueryNextAdrId,
    QueryOrphanDocs,
    QueryNodesDocs,
    QueryNodesCodeWithoutDocs,
    DiffLatest,
}

impl Case {
    fn id(self) -> &'static str {
        match self {
            Self::WatchOnceCold => "watch_once_cold",
            Self::WatchOnceWarm => "watch_once_warm",
            Self::WatchOnceChanged => "watch_once_changed",
            Self::WatchOnceSemanticChanged => "watch_once_semantic_changed",
            Self::Check => "check",
            Self::EnforceCi => "enforce_ci",
            Self::QueryNextAdrId => "query_next_adr_id",
            Self::QueryOrphanDocs => "query_orphan_docs",
            Self::QueryNodesDocs => "query_nodes_docs",
            Self::QueryNodesCodeWithoutDocs => "query_nodes_code_without_docs",
            Self::DiffLatest => "diff_latest",
        }
    }

    fn cache_state(self) -> &'static str {
        match self {
            Self::WatchOnceWarm
            | Self::WatchOnceChanged
            | Self::WatchOnceSemanticChanged
            | Self::DiffLatest => "warm",
            _ => "cold",
        }
    }

    fn args(self) -> &'static [&'static str] {
        match self {
            Self::WatchOnceCold
            | Self::WatchOnceWarm
            | Self::WatchOnceChanged
            | Self::WatchOnceSemanticChanged => &["watch", "--once"],
            Self::Check => &["check"],
            Self::EnforceCi => &["enforce", "--stage", "ci"],
            Self::QueryNextAdrId => &["query", "next-adr-id"],
            Self::QueryOrphanDocs => &["query", "orphan-docs"],
            Self::QueryNodesDocs => &["query", "nodes", "--kind", "doc"],
            Self::QueryNodesCodeWithoutDocs => {
                &["query", "nodes", "--kind", "code", "--without-docs"]
            }
            Self::DiffLatest => &["query", "diff", "latest", "latest"],
        }
    }

    fn needs_seed(self) -> bool {
        matches!(
            self,
            Self::WatchOnceWarm
                | Self::WatchOnceChanged
                | Self::WatchOnceSemanticChanged
                | Self::DiffLatest
        )
    }

    fn needs_mutation(self) -> bool {
        self == Self::WatchOnceChanged
    }

    fn needs_semantic_mutation(self) -> bool {
        self == Self::WatchOnceSemanticChanged
    }

    fn snapshot_history(self) -> usize {
        1
    }
}

const ALL_CASES: [Case; 11] = [
    Case::WatchOnceCold,
    Case::WatchOnceWarm,
    Case::WatchOnceChanged,
    Case::WatchOnceSemanticChanged,
    Case::Check,
    Case::EnforceCi,
    Case::QueryNextAdrId,
    Case::QueryOrphanDocs,
    Case::QueryNodesDocs,
    Case::QueryNodesCodeWithoutDocs,
    Case::DiffLatest,
];

#[derive(Debug, Clone, Serialize)]
struct MachineIdentity {
    os: String,
    release: String,
    architecture: String,
    processor: String,
    rustc_verbose: String,
    digest: String,
}

#[derive(Debug, Clone, Serialize)]
struct ManifestIdentity {
    id: String,
    tier: String,
    path: String,
    result_path: String,
    digest: String,
    observed_repository: String,
    observed_revision: String,
}

#[derive(Debug, Clone, Serialize)]
struct RunIdentity {
    schema: &'static str,
    run_id: String,
    started_at_utc: String,
    repository_root: String,
    revision: String,
    dirty: bool,
    binary: String,
    binary_digest: String,
    profile: String,
    samples: usize,
    machine: MachineIdentity,
    manifests: Vec<ManifestIdentity>,
    cases: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SampleRow {
    schema: &'static str,
    run_id: String,
    workload: String,
    workload_digest: String,
    generated_digest: String,
    case: String,
    cache_state: String,
    sample: usize,
    profile: String,
    binary_digest: String,
    machine_digest: String,
    exit_status: i32,
    real_seconds: f64,
    user_seconds: Option<f64>,
    system_seconds: Option<f64>,
    stdout_digest: String,
    stderr_digest: String,
    stdout_path: String,
    stderr_path: String,
    state_digest: Option<String>,
    snapshot_hash: Option<String>,
    source_graph_digest: Option<String>,
}

#[derive(Debug, Serialize)]
struct MetricSummary {
    minimum: f64,
    median: f64,
    maximum: f64,
    median_absolute_deviation: f64,
}

#[derive(Debug, Serialize)]
struct CaseSummary {
    workload: String,
    workload_digest: String,
    case: String,
    cache_state: String,
    successful_samples: usize,
    failed_samples: usize,
    real_seconds: Option<MetricSummary>,
    user_seconds: Option<MetricSummary>,
    system_seconds: Option<MetricSummary>,
}

#[derive(Debug, Serialize)]
struct SummaryDocument<'a> {
    schema: &'static str,
    run: &'a RunIdentity,
    cases: Vec<CaseSummary>,
}

fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("criv-perf: {error}");
        std::process::exit(1);
    }
}

fn run(mut args: Args) -> Result<(), String> {
    validate_args(&args)?;
    let repository_root = fs::canonicalize(&args.repository_root).map_err(display_error)?;
    let binary = fs::canonicalize(&args.binary).map_err(|error| {
        format!(
            "failed to resolve binary {}: {error}",
            args.binary.display()
        )
    })?;
    validate_binary(&binary)?;
    if args.manifest.is_empty() {
        args.manifest = vec![
            repository_root.join("fixtures/performance/barrs-small.toml"),
            repository_root.join("fixtures/performance/criv-medium.toml"),
        ];
    }
    let manifests = args
        .manifest
        .iter()
        .map(|path| {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                repository_root.join(path)
            };
            LoadedManifest::load(&path)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let cases = if args.cases.is_empty() {
        ALL_CASES.to_vec()
    } else {
        let mut cases = args.cases.clone();
        cases.sort();
        cases.dedup();
        cases
    };

    let machine = machine_identity()?;
    let binary_digest = file_digest(&binary)?;
    let revision = command_text(&repository_root, "git", &["rev-parse", "HEAD"])
        .unwrap_or_else(|_| "unavailable".into());
    let dirty = command_text(&repository_root, "git", &["status", "--porcelain"])
        .map(|value| !value.trim().is_empty())
        .unwrap_or(true);
    let started_at_utc = command_text(&repository_root, "date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .unwrap_or_else(|_| unix_millis().to_string());
    let run_seed = format!(
        "{}\0{}\0{}\0{}\0{}",
        revision,
        binary_digest,
        machine.digest,
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
        binary: binary.display().to_string(),
        binary_digest: binary_digest.clone(),
        profile: args.profile.clone(),
        samples: args.samples,
        machine,
        manifests: manifests
            .iter()
            .map(|loaded| ManifestIdentity {
                id: loaded.manifest.id.clone(),
                tier: loaded.manifest.tier.clone(),
                path: loaded.path.display().to_string(),
                result_path: format!("manifests/{}.toml", loaded.manifest.id),
                digest: loaded.digest.clone(),
                observed_repository: loaded.manifest.observed_repository.clone(),
                observed_revision: loaded.manifest.observed_revision.clone(),
            })
            .collect(),
        cases: cases.iter().map(|case| case.id().to_string()).collect(),
    };
    let results_root = if args.results_root.is_absolute() {
        args.results_root.clone()
    } else {
        repository_root.join(&args.results_root)
    };
    let result_dir = create_result_dir(&results_root, &run_id)?;
    fs::create_dir(result_dir.join("outputs")).map_err(display_error)?;
    fs::create_dir(result_dir.join("manifests")).map_err(display_error)?;
    for loaded in &manifests {
        fs::write(
            result_dir
                .join("manifests")
                .join(format!("{}.toml", loaded.manifest.id)),
            &loaded.bytes,
        )
        .map_err(display_error)?;
    }
    write_json(result_dir.join("run.json"), &run_identity)?;
    let samples_path = result_dir.join("samples.jsonl");
    let mut raw = BufWriter::new(File::create(&samples_path).map_err(display_error)?);
    let mut rows = Vec::new();
    let mut failed = false;

    for loaded in &manifests {
        for case in &cases {
            run_warmup(&binary, loaded, *case, &run_id)?;
            for sample in 1..=args.samples {
                let root = TempDir::new().map_err(display_error)?;
                let generated = generate(root.path(), &loaded.manifest)?;
                prepare_sample(&binary, root.path(), loaded, &generated, *case, &run_id)?;
                let row = measure_sample(
                    &result_dir,
                    &run_identity,
                    &binary,
                    loaded,
                    &generated,
                    *case,
                    sample,
                    root.path(),
                )?;
                failed |= row.exit_status != 0;
                serde_json::to_writer(&mut raw, &row).map_err(display_error)?;
                raw.write_all(b"\n").map_err(display_error)?;
                raw.flush().map_err(display_error)?;
                println!(
                    "{}\t{}\tsample={}/{}\tstatus={}\treal={:.6}",
                    loaded.manifest.id,
                    case.id(),
                    sample,
                    args.samples,
                    row.exit_status,
                    row.real_seconds
                );
                rows.push(row);
            }
        }
    }

    let summaries = summarize(&rows);
    write_json(
        result_dir.join("summary.json"),
        &SummaryDocument {
            schema: SUMMARY_SCHEMA,
            run: &run_identity,
            cases: summaries,
        },
    )?;
    println!("results\t{}", result_dir.display());
    if failed {
        return Err(format!(
            "one or more measured commands failed; raw evidence is in {}",
            result_dir.display()
        ));
    }
    Ok(())
}

fn validate_args(args: &Args) -> Result<(), String> {
    if args.samples == 0 || (args.samples < 3 && !args.allow_low_samples) {
        return Err(
            "samples must be at least 3 (use --allow-low-samples only for smoke tests)".into(),
        );
    }
    if args.profile.trim().is_empty() {
        return Err("profile must not be empty".into());
    }
    if args.profile != "release" && !args.allow_non_release {
        return Err(
            "performance evidence requires --profile release (use --allow-non-release only for smoke tests)"
                .into(),
        );
    }
    Ok(())
}

fn validate_binary(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(display_error)?;
    if !metadata.is_file() {
        return Err(format!(
            "binary is not an executable file: {}",
            path.display()
        ));
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!(
            "binary is not an executable file: {}",
            path.display()
        ));
    }
    Ok(())
}

fn run_warmup(
    binary: &Path,
    loaded: &LoadedManifest,
    case: Case,
    run_id: &str,
) -> Result<(), String> {
    let root = TempDir::new().map_err(display_error)?;
    let generated = generate(root.path(), &loaded.manifest)?;
    prepare_sample(binary, root.path(), loaded, &generated, case, run_id)?;
    let output = run_criv(binary, root.path(), case.args(), run_id, "warmup", case)?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "warm-up failed for {} {}:\nstdout:\n{}\nstderr:\n{}",
        loaded.manifest.id,
        case.id(),
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn prepare_sample(
    binary: &Path,
    root: &Path,
    loaded: &LoadedManifest,
    generated: &GeneratedWorkload,
    case: Case,
    run_id: &str,
) -> Result<(), String> {
    if case.needs_seed() {
        let output = run_criv(binary, root, &["watch", "--once"], run_id, "seed", case)?;
        if !output.status.success() {
            return Err(format!(
                "cache seed failed for {} {}:\nstdout:\n{}\nstderr:\n{}",
                loaded.manifest.id,
                case.id(),
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        for revision in 2..=case.snapshot_history() {
            append_source_revision(root, generated, revision)?;
            let sample_id = format!("snapshot-seed-{revision}");
            let output = run_criv(binary, root, &["watch", "--once"], run_id, &sample_id, case)?;
            if !output.status.success() {
                return Err(format!(
                    "snapshot-history seed failed for {} {} revision {revision}:\nstdout:\n{}\nstderr:\n{}",
                    loaded.manifest.id,
                    case.id(),
                    String::from_utf8_lossy(&output.stdout).trim(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
        }
    }
    if case.needs_mutation() {
        mutate_sources(root, generated, loaded.manifest.changed_source_files)?;
    }
    if case.needs_semantic_mutation() {
        append_source_revision(root, generated, 2)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn measure_sample(
    result_dir: &Path,
    run: &RunIdentity,
    binary: &Path,
    loaded: &LoadedManifest,
    generated: &GeneratedWorkload,
    case: Case,
    sample: usize,
    root: &Path,
) -> Result<SampleRow, String> {
    let usage_before = child_usage();
    let start = Instant::now();
    let output = run_criv(
        binary,
        root,
        case.args(),
        &run.run_id,
        &sample.to_string(),
        case,
    )?;
    let real_seconds = start.elapsed().as_secs_f64();
    let usage_after = child_usage();
    let (user_seconds, system_seconds) = usage_delta(usage_before, usage_after);
    let prefix = format!("{}-{}-{sample:03}", loaded.manifest.id, case.id());
    let stdout_path = Path::new("outputs").join(format!("{prefix}.stdout"));
    let stderr_path = Path::new("outputs").join(format!("{prefix}.stderr"));
    fs::write(result_dir.join(&stdout_path), &output.stdout).map_err(display_error)?;
    fs::write(result_dir.join(&stderr_path), &output.stderr).map_err(display_error)?;
    Ok(SampleRow {
        schema: SAMPLE_SCHEMA,
        run_id: run.run_id.clone(),
        workload: loaded.manifest.id.clone(),
        workload_digest: loaded.digest.clone(),
        generated_digest: generated.digest.clone(),
        case: case.id().into(),
        cache_state: case.cache_state().into(),
        sample,
        profile: run.profile.clone(),
        binary_digest: run.binary_digest.clone(),
        machine_digest: run.machine.digest.clone(),
        exit_status: output.status.code().unwrap_or(-1),
        real_seconds,
        user_seconds,
        system_seconds,
        stdout_digest: bytes_digest(&output.stdout),
        stderr_digest: bytes_digest(&output.stderr),
        stdout_path: stdout_path.display().to_string(),
        stderr_path: stderr_path.display().to_string(),
        state_digest: optional_file_digest(&root.join(".criv/state.json")),
        snapshot_hash: fs::read_to_string(root.join(".criv/latest"))
            .ok()
            .map(|value| value.trim().to_string()),
        source_graph_digest: optional_file_digest(&root.join(".criv/source-graph.json")),
    })
}

fn run_criv(
    binary: &Path,
    root: &Path,
    args: &[&str],
    run_id: &str,
    sample_id: &str,
    case: Case,
) -> Result<Output, String> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir(root)
        .env("CRIV_PERF_RUN_ID", run_id)
        .env("CRIV_PERF_SAMPLE_ID", sample_id)
        .env("CRIV_PERF_CASE", case.id())
        .env("CRIV_PERF_CACHE_STATE", case.cache_state())
        .env_remove("CRIV_BASE_REF")
        .env_remove("GITHUB_BASE_REF");
    if case == Case::EnforceCi {
        command.env("CRIV_BASE_REF", "HEAD^");
    }
    command
        .output()
        .map_err(|error| format!("failed to execute {}: {error}", binary.display()))
}

fn summarize(rows: &[SampleRow]) -> Vec<CaseSummary> {
    let mut groups = BTreeMap::<(String, String, String, String), Vec<&SampleRow>>::new();
    for row in rows {
        groups
            .entry((
                row.workload.clone(),
                row.workload_digest.clone(),
                row.case.clone(),
                row.cache_state.clone(),
            ))
            .or_default()
            .push(row);
    }
    groups
        .into_iter()
        .map(|((workload, workload_digest, case, cache_state), rows)| {
            let successful = rows
                .iter()
                .copied()
                .filter(|row| row.exit_status == 0)
                .collect::<Vec<_>>();
            CaseSummary {
                workload,
                workload_digest,
                case,
                cache_state,
                successful_samples: successful.len(),
                failed_samples: rows.len() - successful.len(),
                real_seconds: metric(successful.iter().map(|row| row.real_seconds).collect()),
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
            }
        })
        .collect()
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
        maximum: values[values.len() - 1],
        median_absolute_deviation: median(&deviations),
    })
}

fn median(values: &[f64]) -> f64 {
    if values.len() % 2 == 1 {
        values[values.len() / 2]
    } else {
        (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
    }
}

fn create_result_dir(root: &Path, run_id: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(display_error)?;
    for suffix in 0..1000usize {
        let name = if suffix == 0 {
            format!("{}-{run_id}", unix_millis())
        } else {
            format!("{}-{run_id}-{suffix}", unix_millis())
        };
        let path = root.join(name);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err(format!(
        "failed to allocate unique result directory below {}",
        root.display()
    ))
}

fn machine_identity() -> Result<MachineIdentity, String> {
    let cwd = std::env::current_dir().map_err(display_error)?;
    let os = command_text(&cwd, "uname", &["-s"]).unwrap_or_else(|_| std::env::consts::OS.into());
    let release = command_text(&cwd, "uname", &["-r"]).unwrap_or_else(|_| "unknown".into());
    let architecture =
        command_text(&cwd, "uname", &["-m"]).unwrap_or_else(|_| std::env::consts::ARCH.into());
    let processor = command_text(&cwd, "sysctl", &["-n", "machdep.cpu.brand_string"])
        .or_else(|_| read_processor_linux())
        .unwrap_or_else(|_| "unknown".into());
    let rustc_verbose = command_text(&cwd, "rustc", &["--version", "--verbose"])?;
    let text = format!("{os}\0{release}\0{architecture}\0{processor}\0{rustc_verbose}");
    Ok(MachineIdentity {
        os,
        release,
        architecture,
        processor,
        rustc_verbose,
        digest: blake3::hash(text.as_bytes()).to_hex().to_string(),
    })
}

fn read_processor_linux() -> Result<String, String> {
    let contents = fs::read_to_string("/proc/cpuinfo").map_err(display_error)?;
    contents
        .lines()
        .find_map(|line| line.strip_prefix("model name\t: "))
        .map(str::to_string)
        .ok_or_else(|| "processor model unavailable".into())
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

fn optional_file_digest(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|bytes| bytes_digest(&bytes))
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

#[cfg(unix)]
fn child_usage() -> Option<(f64, f64)> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes the provided rusage on a zero return code.
    let status = unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, usage.as_mut_ptr()) };
    if status != 0 {
        return None;
    }
    // SAFETY: the successful getrusage call initialized usage.
    let usage = unsafe { usage.assume_init() };
    Some((
        timeval_seconds(usage.ru_utime),
        timeval_seconds(usage.ru_stime),
    ))
}

#[cfg(unix)]
fn timeval_seconds(value: libc::timeval) -> f64 {
    value.tv_sec as f64 + value.tv_usec as f64 / 1_000_000.0
}

#[cfg(not(unix))]
fn child_usage() -> Option<(f64, f64)> {
    None
}

fn usage_delta(
    before: Option<(f64, f64)>,
    after: Option<(f64, f64)>,
) -> (Option<f64>, Option<f64>) {
    match (before, after) {
        (Some(before), Some(after)) => (
            Some((after.0 - before.0).max(0.0)),
            Some((after.1 - before.1).max(0.0)),
        ),
        _ => (None, None),
    }
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_and_mad_cover_odd_and_even_samples() {
        let odd = metric(vec![1.0, 2.0, 100.0]).unwrap();
        assert_eq!(odd.minimum, 1.0);
        assert_eq!(odd.median, 2.0);
        assert_eq!(odd.maximum, 100.0);
        assert_eq!(odd.median_absolute_deviation, 1.0);

        let even = metric(vec![1.0, 3.0, 5.0, 7.0]).unwrap();
        assert_eq!(even.median, 4.0);
        assert_eq!(even.median_absolute_deviation, 2.0);
    }

    #[test]
    fn unique_result_directories_never_reuse_previous_runs() {
        let root = tempfile::TempDir::new().unwrap();
        let first = create_result_dir(root.path(), "same").unwrap();
        let second = create_result_dir(root.path(), "same").unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn semantic_change_case_seeds_then_changes_one_state_partition() {
        assert!(Case::WatchOnceSemanticChanged.needs_seed());
        assert!(Case::WatchOnceSemanticChanged.needs_semantic_mutation());
        assert_eq!(Case::WatchOnceSemanticChanged.cache_state(), "warm");
        assert_eq!(Case::WatchOnceSemanticChanged.args(), &["watch", "--once"]);
    }

    #[test]
    fn undocumented_code_case_measures_the_reverse_reference_query() {
        assert_eq!(
            Case::QueryNodesCodeWithoutDocs.id(),
            "query_nodes_code_without_docs"
        );
        assert_eq!(
            Case::QueryNodesCodeWithoutDocs.args(),
            &["query", "nodes", "--kind", "code", "--without-docs"]
        );
        assert_eq!(Case::QueryNodesCodeWithoutDocs.cache_state(), "cold");
    }
}
