use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};

const RUN_SCHEMA: &str = "criv.discovery-command-run.v1";
const SAMPLE_SCHEMA: &str = "criv.discovery-command-sample.v1";
const SUMMARY_SCHEMA: &str = "criv.discovery-command-summary.v1";
const INVENTORY_SCHEMA: &str = "criv.discovery-inventory.v1";

#[derive(Debug, Parser)]
#[command(
    name = "criv-discovery-commands",
    about = "Measure official criv commands on isolated observed-workload snapshots"
)]
struct Args {
    /// Official criv executable to measure.
    #[arg(long)]
    binary: PathBuf,
    /// Human-readable release artifact identity.
    #[arg(long, default_value = "official-criv-0.9.0-artifact")]
    binary_label: String,
    /// Strict snapshot helper executable.
    #[arg(long)]
    snapshot_executable: PathBuf,
    /// Read-only golden observed workload.
    #[arg(long)]
    workload_root: PathBuf,
    /// Full local workload inventory.
    #[arg(long)]
    workload_inventory: PathBuf,
    /// Parent for one disposable snapshot at a time.
    #[arg(long)]
    sample_root: PathBuf,
    /// Parent for preserved result directories.
    #[arg(long, default_value = "target/discovery-command-results")]
    results_root: PathBuf,
    /// Existing source file to modify for the changed-source case.
    #[arg(long)]
    source_mutation_path: Option<PathBuf>,
    /// Existing non-ADR Markdown file to modify for the changed-Markdown case.
    #[arg(long)]
    markdown_mutation_path: Option<PathBuf>,
    /// Configured Source directory used for live create, rename, and delete.
    #[arg(long)]
    live_mutation_directory: Option<PathBuf>,
    /// Command case to measure. Repeat to select more than one.
    #[arg(long = "case", value_enum)]
    cases: Vec<Case>,
    /// Number of recorded samples per case and attempt.
    #[arg(long, default_value_t = 5)]
    samples: usize,
    /// Permit fewer than five samples for smoke tests only.
    #[arg(long)]
    allow_low_samples: bool,
    /// Startup and convergence timeout in seconds.
    #[arg(long, default_value_t = 120)]
    timeout_seconds: u64,
    /// Required free space before each snapshot.
    #[arg(long, default_value_t = 30)]
    minimum_free_gib: u64,
    /// Maximum volume allocation observed while the strict snapshot is made.
    #[arg(long, default_value_t = 20)]
    maximum_snapshot_allocation_gib: u64,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
enum Case {
    WatchOnceCold,
    WatchOnceWarm,
    WatchLiveReady,
    CheckFull,
    CheckChangedSource,
    CheckChangedMarkdown,
}

impl Case {
    fn id(self) -> &'static str {
        match self {
            Self::WatchOnceCold => "watch_once_cold",
            Self::WatchOnceWarm => "watch_once_warm",
            Self::WatchLiveReady => "watch_live_ready",
            Self::CheckFull => "check_full",
            Self::CheckChangedSource => "check_changed_source",
            Self::CheckChangedMarkdown => "check_changed_markdown",
        }
    }

    fn cache_state(self) -> &'static str {
        match self {
            Self::WatchOnceWarm | Self::CheckChangedSource | Self::CheckChangedMarkdown => "warm",
            _ => "cold",
        }
    }

    fn needs_seed(self) -> bool {
        matches!(
            self,
            Self::WatchOnceWarm | Self::CheckChangedSource | Self::CheckChangedMarkdown
        )
    }
}

const ALL_CASES: [Case; 6] = [
    Case::WatchOnceCold,
    Case::WatchOnceWarm,
    Case::WatchLiveReady,
    Case::CheckFull,
    Case::CheckChangedSource,
    Case::CheckChangedMarkdown,
];

#[derive(Debug, Deserialize)]
struct WorkloadInventoryHeader {
    schema: String,
    workload_id: String,
    workload_digest: String,
}

#[derive(Debug, Serialize)]
struct RunIdentity {
    schema: &'static str,
    run_id: String,
    started_at_utc: String,
    binary_label: String,
    binary: String,
    binary_digest: String,
    binary_version: String,
    harness: String,
    harness_digest: String,
    snapshot_executable: String,
    snapshot_digest: String,
    workload_root: String,
    workload_id: String,
    workload_digest: String,
    operating_system: String,
    architecture: String,
    processor: String,
    rustc_verbose: String,
    machine_digest: String,
    snapshot_mode: &'static str,
    minimum_free_gib: u64,
    maximum_snapshot_allocation_gib: u64,
    samples: usize,
    timeout_seconds: u64,
    cases: Vec<&'static str>,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct StateIdentity {
    state_digest: String,
    latest_snapshot: String,
    snapshot_digest: Option<String>,
    source_graph_digest: Option<String>,
    source_paths: usize,
    source_path_digest: String,
    vault_markdown_paths: usize,
    vault_markdown_path_digest: String,
    vault_c4_paths: usize,
    vault_c4_path_digest: String,
}

struct StateObservation {
    identity: StateIdentity,
    source_paths: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ConvergenceStep {
    operation: &'static str,
    elapsed_seconds: f64,
    state_digest: String,
}

#[derive(Debug, Serialize)]
struct SampleRow {
    schema: &'static str,
    run_id: String,
    workload_id: String,
    workload_digest: String,
    binary_label: String,
    binary_digest: String,
    case: String,
    cache_state: String,
    attempt: usize,
    sample: usize,
    successful: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_status: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    real_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publication_ready_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    peak_rss_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ready_rss_bytes: Option<u64>,
    stdout_digest: String,
    stderr_digest: String,
    stdout_path: String,
    stderr_path: String,
    snapshot_receipt_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    staged_patch_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state_before: Option<StateIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state_after: Option<StateIdentity>,
    state_unchanged: Option<bool>,
    source_graph_unchanged: Option<bool>,
    convergence_steps: Vec<ConvergenceStep>,
    live_matches_one_shot: Option<bool>,
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
    case: String,
    attempt: usize,
    successful_samples: usize,
    failed_samples: usize,
    real_seconds: Option<MetricSummary>,
    publication_ready_seconds: Option<MetricSummary>,
    convergence_seconds: Option<MetricSummary>,
    peak_rss_bytes: Option<MetricSummary>,
    ready_rss_bytes: Option<MetricSummary>,
    relative_mad: Option<f64>,
    unstable: bool,
}

#[derive(Debug, Serialize)]
struct SummaryDocument<'a> {
    schema: &'static str,
    run: &'a RunIdentity,
    warmup_failures: Vec<String>,
    attempts: Vec<AttemptSummary>,
}

struct ProcessOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    user_seconds: Option<f64>,
    system_seconds: Option<f64>,
    peak_rss_bytes: Option<u64>,
}

struct CaseOutcome {
    successful: bool,
    error: Option<String>,
    exit_status: Option<i32>,
    real_seconds: Option<f64>,
    publication_ready_seconds: Option<f64>,
    user_seconds: Option<f64>,
    system_seconds: Option<f64>,
    peak_rss_bytes: Option<u64>,
    ready_rss_bytes: Option<u64>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    staged_patch_digest: Option<String>,
    state_before: Option<StateIdentity>,
    state_after: Option<StateIdentity>,
    state_unchanged: Option<bool>,
    source_graph_unchanged: Option<bool>,
    convergence_steps: Vec<ConvergenceStep>,
    live_matches_one_shot: Option<bool>,
}

struct DisposableSnapshot {
    path: PathBuf,
}

impl Drop for DisposableSnapshot {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            eprintln!(
                "criv-discovery-commands: failed to remove disposable snapshot {}: {error}",
                self.path.display()
            );
        }
    }
}

fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("criv-discovery-commands: {error}");
        std::process::exit(1);
    }
}

fn run(mut args: Args) -> Result<(), String> {
    validate_args(&args)?;
    let binary = canonical_file(&args.binary)?;
    let snapshot_executable = canonical_file(&args.snapshot_executable)?;
    let workload_root = canonical_directory(&args.workload_root)?;
    let sample_root = canonical_directory(&args.sample_root)?;
    let inventory = read_inventory(&args.workload_inventory)?;
    let cases = if args.cases.is_empty() {
        ALL_CASES.to_vec()
    } else {
        args.cases.sort();
        args.cases.dedup();
        std::mem::take(&mut args.cases)
    };
    validate_case_inputs(&args, &workload_root, &cases)?;

    let binary_digest = file_digest(&binary)?;
    let harness = std::env::current_exe().map_err(display_error)?;
    let harness_digest = file_digest(&harness)?;
    let snapshot_digest = file_digest(&snapshot_executable)?;
    let started_at_utc = command_text(Path::new("."), "date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .unwrap_or_else(|_| unix_millis().to_string());
    let binary_version = command_text(
        Path::new("."),
        binary.to_str().unwrap_or("criv"),
        &["--version"],
    )
    .unwrap_or_else(|_| "unavailable".into());
    let run_seed = format!(
        "{}\0{}\0{}\0{}\0{}",
        inventory.workload_digest,
        binary_digest,
        snapshot_digest,
        started_at_utc,
        std::process::id()
    );
    let run_id = blake3::hash(run_seed.as_bytes()).to_hex()[..16].to_string();
    let operating_system = command_text(Path::new("."), "uname", &["-sr"])
        .unwrap_or_else(|_| std::env::consts::OS.into());
    let architecture = std::env::consts::ARCH.to_string();
    let processor = processor_identity(Path::new("."));
    let rustc_verbose = command_text(Path::new("."), "rustc", &["--version", "--verbose"])
        .unwrap_or_else(|_| "unavailable".into());
    let machine_digest = bytes_digest(
        format!("{operating_system}\0{architecture}\0{processor}\0{rustc_verbose}").as_bytes(),
    );
    let run_identity = RunIdentity {
        schema: RUN_SCHEMA,
        run_id: run_id.clone(),
        started_at_utc,
        binary_label: args.binary_label.clone(),
        binary: binary.display().to_string(),
        binary_digest: binary_digest.clone(),
        binary_version,
        harness: harness.display().to_string(),
        harness_digest,
        snapshot_executable: snapshot_executable.display().to_string(),
        snapshot_digest,
        workload_root: workload_root.display().to_string(),
        workload_id: inventory.workload_id,
        workload_digest: inventory.workload_digest,
        operating_system,
        architecture,
        processor,
        rustc_verbose,
        machine_digest,
        snapshot_mode: "one_strict_snapshot_reset_between_samples",
        minimum_free_gib: args.minimum_free_gib,
        maximum_snapshot_allocation_gib: args.maximum_snapshot_allocation_gib,
        samples: args.samples,
        timeout_seconds: args.timeout_seconds,
        cases: cases.iter().map(|case| case.id()).collect(),
    };

    let result_dir = create_result_dir(&args.results_root, &run_id)?;
    fs::create_dir(result_dir.join("outputs")).map_err(display_error)?;
    let sample_run_root = sample_root.join(format!("criv-discovery-{run_id}"));
    fs::create_dir(&sample_run_root).map_err(display_error)?;
    let (snapshot, snapshot_receipt) = create_snapshot(
        &snapshot_executable,
        &workload_root,
        &sample_run_root.join("workload"),
        args.minimum_free_gib,
        args.maximum_snapshot_allocation_gib,
    )?;
    let snapshot_receipt_digest = bytes_digest(&snapshot_receipt);
    write_json(result_dir.join("run.json"), &run_identity)?;
    let mut raw =
        BufWriter::new(File::create(result_dir.join("samples.jsonl")).map_err(display_error)?);
    let mut rows = Vec::new();
    let mut warmup_failures = Vec::new();
    for case in cases {
        if let Err(error) = run_warmup(&args, &binary, &snapshot.path, case) {
            eprintln!("criv-discovery-commands: {error}");
            warmup_failures.push(error);
        }
        let first = run_attempt(
            &args,
            &binary,
            &snapshot.path,
            &snapshot_receipt_digest,
            &result_dir,
            &run_identity,
            case,
            1,
            &mut raw,
        )?;
        let unstable = summarize_attempt(case, 1, &first).unstable;
        rows.extend(first);
        if unstable {
            let second = run_attempt(
                &args,
                &binary,
                &snapshot.path,
                &snapshot_receipt_digest,
                &result_dir,
                &run_identity,
                case,
                2,
                &mut raw,
            )?;
            rows.extend(second);
        }
    }
    raw.flush().map_err(display_error)?;
    drop(snapshot);
    fs::remove_dir(&sample_run_root).map_err(display_error)?;

    let mut summaries = Vec::new();
    for case in ALL_CASES {
        for attempt in 1..=2 {
            let selected = rows
                .iter()
                .filter(|row| row.case == case.id() && row.attempt == attempt)
                .collect::<Vec<_>>();
            if !selected.is_empty() {
                summaries.push(summarize_attempt_refs(case, attempt, &selected));
            }
        }
    }
    let summary = SummaryDocument {
        schema: SUMMARY_SCHEMA,
        run: &run_identity,
        warmup_failures,
        attempts: summaries,
    };
    write_json(result_dir.join("summary.json"), &summary)?;
    println!("{}", result_dir.display());

    let failed = rows.iter().any(|row| !row.successful);
    let unstable_output = ensure_stable_outputs(&rows).err();
    match (failed, unstable_output) {
        (true, Some(error)) => {
            return Err(format!(
                "one or more command samples failed and output identity was unstable ({error}); raw evidence was preserved"
            ));
        }
        (true, None) => {
            return Err("one or more command samples failed; raw evidence was preserved".into());
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

fn validate_args(args: &Args) -> Result<(), String> {
    if args.samples == 0 || (args.samples < 5 && !args.allow_low_samples) {
        return Err(
            "samples must be at least 5; use --allow-low-samples only for smoke tests".into(),
        );
    }
    if args.timeout_seconds == 0 {
        return Err("timeout-seconds must be positive".into());
    }
    if args.minimum_free_gib == 0 || args.maximum_snapshot_allocation_gib == 0 {
        return Err("snapshot space limits must be positive".into());
    }
    if args.binary_label.trim().is_empty() {
        return Err("binary-label must not be empty".into());
    }
    Ok(())
}

fn validate_case_inputs(args: &Args, root: &Path, cases: &[Case]) -> Result<(), String> {
    for (case, path) in [
        (Case::CheckChangedSource, &args.source_mutation_path),
        (Case::CheckChangedMarkdown, &args.markdown_mutation_path),
    ] {
        if cases.contains(&case) {
            let path = path
                .as_ref()
                .ok_or_else(|| format!("{} requires its mutation path option", case.id()))?;
            validate_relative_path(path)?;
            if !root.join(path).is_file() {
                return Err(format!(
                    "{} mutation path is not a file: {}",
                    case.id(),
                    path.display()
                ));
            }
        }
    }
    if cases.contains(&Case::WatchLiveReady) {
        let path = args
            .live_mutation_directory
            .as_ref()
            .ok_or_else(|| "watch_live_ready requires --live-mutation-directory".to_string())?;
        validate_relative_path(path)?;
        if !root.join(path).is_dir() {
            return Err(format!(
                "live mutation directory does not exist: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<(), String> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "mutation path must be a normalized repository-relative path: {}",
            path.display()
        ));
    }
    Ok(())
}

fn read_inventory(path: &Path) -> Result<WorkloadInventoryHeader, String> {
    let input = File::open(path)
        .map_err(|error| format!("failed to open inventory {}: {error}", path.display()))?;
    let inventory: WorkloadInventoryHeader =
        serde_json::from_reader(BufReader::new(input)).map_err(display_error)?;
    if inventory.schema != INVENTORY_SCHEMA {
        return Err(format!(
            "unsupported workload inventory schema {}",
            inventory.schema
        ));
    }
    if inventory.workload_digest.len() != 64
        || !inventory
            .workload_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("inventory contains an invalid workload digest".into());
    }
    Ok(inventory)
}

fn run_warmup(args: &Args, binary: &Path, snapshot_root: &Path, case: Case) -> Result<(), String> {
    restore_snapshot(args, snapshot_root)?;
    prepare_case(args, binary, snapshot_root, case)?;
    let outcome = execute_case(args, binary, snapshot_root, case)?;
    if outcome.successful {
        Ok(())
    } else {
        Err(format!(
            "warm-up failed for {}: {}\nstdout:\n{}\nstderr:\n{}",
            case.id(),
            outcome.error.unwrap_or_else(|| "command failed".into()),
            String::from_utf8_lossy(&outcome.stdout).trim(),
            String::from_utf8_lossy(&outcome.stderr).trim()
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn run_attempt(
    args: &Args,
    binary: &Path,
    snapshot_root: &Path,
    snapshot_receipt_digest: &str,
    result_dir: &Path,
    run: &RunIdentity,
    case: Case,
    attempt: usize,
    raw: &mut BufWriter<File>,
) -> Result<Vec<SampleRow>, String> {
    let mut rows = Vec::with_capacity(args.samples);
    for sample in 1..=args.samples {
        let setup = restore_snapshot(args, snapshot_root)
            .and_then(|()| prepare_case(args, binary, snapshot_root, case));
        let outcome = match setup {
            Ok(()) => execute_case(args, binary, snapshot_root, case)?,
            Err(error) => failed_outcome(error),
        };
        let prefix = format!("{}-attempt-{attempt}-sample-{sample:03}", case.id());
        let stdout_path = Path::new("outputs").join(format!("{prefix}.stdout"));
        let stderr_path = Path::new("outputs").join(format!("{prefix}.stderr"));
        fs::write(result_dir.join(&stdout_path), &outcome.stdout).map_err(display_error)?;
        fs::write(result_dir.join(&stderr_path), &outcome.stderr).map_err(display_error)?;
        let row = SampleRow {
            schema: SAMPLE_SCHEMA,
            run_id: run.run_id.clone(),
            workload_id: run.workload_id.clone(),
            workload_digest: run.workload_digest.clone(),
            binary_label: run.binary_label.clone(),
            binary_digest: run.binary_digest.clone(),
            case: case.id().into(),
            cache_state: case.cache_state().into(),
            attempt,
            sample,
            successful: outcome.successful,
            error: outcome.error,
            exit_status: outcome.exit_status,
            real_seconds: outcome.real_seconds,
            publication_ready_seconds: outcome.publication_ready_seconds,
            user_seconds: outcome.user_seconds,
            system_seconds: outcome.system_seconds,
            peak_rss_bytes: outcome.peak_rss_bytes,
            ready_rss_bytes: outcome.ready_rss_bytes,
            stdout_digest: bytes_digest(&outcome.stdout),
            stderr_digest: bytes_digest(&outcome.stderr),
            stdout_path: stdout_path.display().to_string(),
            stderr_path: stderr_path.display().to_string(),
            snapshot_receipt_digest: snapshot_receipt_digest.into(),
            staged_patch_digest: outcome.staged_patch_digest,
            state_before: outcome.state_before,
            state_after: outcome.state_after,
            state_unchanged: outcome.state_unchanged,
            source_graph_unchanged: outcome.source_graph_unchanged,
            convergence_steps: outcome.convergence_steps,
            live_matches_one_shot: outcome.live_matches_one_shot,
        };
        serde_json::to_writer(&mut *raw, &row).map_err(display_error)?;
        raw.write_all(b"\n").map_err(display_error)?;
        rows.push(row);
    }
    Ok(rows)
}

fn create_snapshot(
    executable: &Path,
    source: &Path,
    destination: &Path,
    minimum_free_gib: u64,
    maximum_allocation_gib: u64,
) -> Result<(DisposableSnapshot, Vec<u8>), String> {
    let output = Command::new(executable)
        .args(["--source"])
        .arg(source)
        .arg("--destination")
        .arg(destination)
        .args(["--minimum-free-gib", &minimum_free_gib.to_string()])
        .args([
            "--maximum-allocation-gib",
            &maximum_allocation_gib.to_string(),
        ])
        .output()
        .map_err(display_error)?;
    if !output.status.success() {
        return Err(format!(
            "snapshot helper failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok((
        DisposableSnapshot {
            path: destination.into(),
        },
        output.stdout,
    ))
}

fn restore_snapshot(args: &Args, root: &Path) -> Result<(), String> {
    let reset = Command::new("git")
        .args(["reset", "--hard", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(display_error)?;
    if !reset.status.success() {
        return Err(format!(
            "failed to restore disposable snapshot: {}",
            String::from_utf8_lossy(&reset.stderr).trim()
        ));
    }
    remove_local_state(root)?;
    if let Some(directory) = &args.live_mutation_directory {
        for name in [
            "criv_discovery_watch_probe.rs",
            "criv_discovery_watch_probe_renamed.rs",
        ] {
            match fs::remove_file(root.join(directory).join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(display_error(error)),
            }
        }
    }
    Ok(())
}

fn prepare_case(args: &Args, binary: &Path, root: &Path, case: Case) -> Result<(), String> {
    remove_local_state(root)?;
    if case.needs_seed() {
        let seed = run_short(binary, root, &["watch", "--once"])?;
        if !seed.status.success() {
            return Err(format!(
                "untimed seed failed: {}",
                String::from_utf8_lossy(&seed.stderr).trim()
            ));
        }
    }
    match case {
        Case::CheckChangedSource => stage_mutation(
            root,
            args.source_mutation_path.as_ref().unwrap(),
            "\n// criv discovery benchmark\n",
        )?,
        Case::CheckChangedMarkdown => stage_mutation(
            root,
            args.markdown_mutation_path.as_ref().unwrap(),
            "\n#bad\n",
        )?,
        _ => {}
    }
    Ok(())
}

fn remove_local_state(root: &Path) -> Result<(), String> {
    let path = root.join(".criv");
    match fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "failed to remove disposable local state {}: {error}",
            path.display()
        )),
    }
}

fn stage_mutation(root: &Path, relative: &Path, suffix: &str) -> Result<(), String> {
    let path = root.join(relative);
    let mut contents = fs::read(&path).map_err(display_error)?;
    contents.extend_from_slice(suffix.as_bytes());
    fs::write(&path, contents).map_err(display_error)?;
    let output = Command::new("git")
        .arg("add")
        .arg("--")
        .arg(relative)
        .current_dir(root)
        .output()
        .map_err(display_error)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "failed to stage {}: {}",
            relative.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn execute_case(
    args: &Args,
    binary: &Path,
    root: &Path,
    case: Case,
) -> Result<CaseOutcome, String> {
    if case == Case::WatchLiveReady {
        return run_live(args, binary, root);
    }
    let command_args: &[&str] = match case {
        Case::WatchOnceCold | Case::WatchOnceWarm => &["watch", "--once"],
        Case::CheckFull => &["check", "--format", "json"],
        Case::CheckChangedSource | Case::CheckChangedMarkdown => {
            &["check", "--changed", "--format", "json"]
        }
        Case::WatchLiveReady => unreachable!(),
    };
    let state_before = read_state_identity(root).ok();
    let staged_patch_digest = matches!(case, Case::CheckChangedSource | Case::CheckChangedMarkdown)
        .then(|| command_bytes(root, "git", &["diff", "--cached", "--binary"]))
        .transpose()?
        .map(|bytes| bytes_digest(&bytes));
    let start = Instant::now();
    let output = run_short(binary, root, command_args)?;
    let elapsed = start.elapsed().as_secs_f64();
    let state_after = read_state_identity(root).ok();
    let state_unchanged = state_before
        .as_ref()
        .zip(state_after.as_ref())
        .map(|(before, after)| published_state_unchanged(before, after));
    let source_graph_unchanged = state_before
        .as_ref()
        .zip(state_after.as_ref())
        .map(|(before, after)| before.source_graph_digest == after.source_graph_digest);
    let command_succeeded = match case {
        Case::CheckFull => {
            full_check_completed(output.status.code(), &output.stdout, &output.stderr)
        }
        Case::CheckChangedMarkdown => changed_markdown_reported_expected_diagnostic(
            &output,
            args.markdown_mutation_path.as_ref().unwrap(),
        ),
        _ => output.status.success(),
    };
    let successful = command_succeeded
        && (!matches!(case, Case::CheckChangedSource | Case::CheckChangedMarkdown)
            || state_unchanged == Some(true));
    Ok(CaseOutcome {
        successful,
        error: (!successful).then(|| "command failed or changed seeded State".into()),
        exit_status: output.status.code(),
        real_seconds: Some(elapsed),
        publication_ready_seconds: None,
        user_seconds: output.user_seconds,
        system_seconds: output.system_seconds,
        peak_rss_bytes: output.peak_rss_bytes,
        ready_rss_bytes: None,
        stdout: output.stdout,
        stderr: output.stderr,
        staged_patch_digest,
        state_before,
        state_after,
        state_unchanged,
        source_graph_unchanged,
        convergence_steps: vec![],
        live_matches_one_shot: None,
    })
}

fn run_live(args: &Args, binary: &Path, root: &Path) -> Result<CaseOutcome, String> {
    let mut watch = LiveWatch::spawn(binary, root)?;
    let timeout = Duration::from_secs(args.timeout_seconds);
    let started = Instant::now();
    let (publication_ready_seconds, ready_seconds) = match watch.wait_until_ready(timeout, started)
    {
        Ok(readiness) => readiness,
        Err(error) => {
            let stopped = watch.stop()?;
            return Ok(failed_live_outcome(
                error,
                stopped,
                None,
                None,
                None,
                read_state_identity(root).ok(),
                None,
                vec![],
            ));
        }
    };
    let ready_rss_bytes = current_rss_bytes(watch.child.id());
    let initial = match read_state_identity(root) {
        Ok(identity) => identity,
        Err(error) => {
            let stopped = watch.stop()?;
            return Ok(failed_live_outcome(
                format!("failed to read State at live readiness: {error}"),
                stopped,
                Some(ready_seconds),
                Some(publication_ready_seconds),
                ready_rss_bytes,
                None,
                None,
                vec![],
            ));
        }
    };
    let mutation_directory = args.live_mutation_directory.as_ref().unwrap();
    let first = mutation_directory.join("criv_discovery_watch_probe.rs");
    let renamed = mutation_directory.join("criv_discovery_watch_probe_renamed.rs");
    let mut steps = Vec::new();
    let convergence = (|| -> Result<StateIdentity, String> {
        let operation = Instant::now();
        fs::write(root.join(&first), "pub fn discovery_watch_probe() {}\n")
            .map_err(display_error)?;
        let created = watch
            .wait_for_source_paths(timeout, root, |paths| paths.contains(&path_text(&first)))?;
        steps.push(ConvergenceStep {
            operation: "create",
            elapsed_seconds: operation.elapsed().as_secs_f64(),
            state_digest: created.state_digest,
        });

        let operation = Instant::now();
        fs::rename(root.join(&first), root.join(&renamed)).map_err(display_error)?;
        let renamed_state = watch.wait_for_source_paths(timeout, root, |paths| {
            paths.contains(&path_text(&renamed)) && !paths.contains(&path_text(&first))
        })?;
        steps.push(ConvergenceStep {
            operation: "rename",
            elapsed_seconds: operation.elapsed().as_secs_f64(),
            state_digest: renamed_state.state_digest,
        });

        let operation = Instant::now();
        fs::remove_file(root.join(&renamed)).map_err(display_error)?;
        let deleted = watch.wait_for_source_paths(timeout, root, |paths| {
            !paths.contains(&path_text(&renamed)) && !paths.contains(&path_text(&first))
        })?;
        steps.push(ConvergenceStep {
            operation: "delete",
            elapsed_seconds: operation.elapsed().as_secs_f64(),
            state_digest: deleted.state_digest.clone(),
        });
        Ok(deleted)
    })();
    let stopped = watch.stop()?;
    let deleted = match convergence {
        Ok(identity) => identity,
        Err(error) => {
            return Ok(failed_live_outcome(
                error,
                stopped,
                Some(ready_seconds),
                Some(publication_ready_seconds),
                ready_rss_bytes,
                Some(initial),
                read_state_identity(root).ok(),
                steps,
            ));
        }
    };
    let one_shot = match run_short(binary, root, &["watch", "--once"]) {
        Ok(output) => output,
        Err(error) => {
            return Ok(failed_live_outcome(
                format!("failed to run one-shot convergence check: {error}"),
                stopped,
                Some(ready_seconds),
                Some(publication_ready_seconds),
                ready_rss_bytes,
                Some(initial),
                Some(deleted),
                steps,
            ));
        }
    };
    let one_shot_state = match read_state_identity(root) {
        Ok(identity) => identity,
        Err(error) => {
            return Ok(failed_live_outcome(
                format!("failed to read one-shot convergence State: {error}"),
                stopped,
                Some(ready_seconds),
                Some(publication_ready_seconds),
                ready_rss_bytes,
                Some(initial),
                Some(deleted),
                steps,
            ));
        }
    };
    let matches = one_shot.status.success() && deleted == one_shot_state && initial == deleted;
    Ok(CaseOutcome {
        successful: matches,
        error: (!matches).then(|| "live watch did not converge with one-shot State".into()),
        exit_status: stopped.status.code(),
        real_seconds: Some(ready_seconds),
        publication_ready_seconds: Some(publication_ready_seconds),
        user_seconds: stopped.user_seconds,
        system_seconds: stopped.system_seconds,
        peak_rss_bytes: stopped.peak_rss_bytes,
        ready_rss_bytes,
        stdout: stopped.stdout,
        stderr: stopped.stderr,
        staged_patch_digest: None,
        state_before: Some(initial),
        state_after: Some(deleted),
        state_unchanged: None,
        source_graph_unchanged: None,
        convergence_steps: steps,
        live_matches_one_shot: Some(matches),
    })
}

#[allow(clippy::too_many_arguments)]
fn failed_live_outcome(
    error: String,
    stopped: ProcessOutput,
    ready_seconds: Option<f64>,
    publication_ready_seconds: Option<f64>,
    ready_rss_bytes: Option<u64>,
    state_before: Option<StateIdentity>,
    state_after: Option<StateIdentity>,
    convergence_steps: Vec<ConvergenceStep>,
) -> CaseOutcome {
    CaseOutcome {
        successful: false,
        error: Some(error),
        exit_status: stopped.status.code(),
        real_seconds: ready_seconds,
        publication_ready_seconds,
        user_seconds: stopped.user_seconds,
        system_seconds: stopped.system_seconds,
        peak_rss_bytes: stopped.peak_rss_bytes,
        ready_rss_bytes,
        stdout: stopped.stdout,
        stderr: stopped.stderr,
        staged_patch_digest: None,
        state_before,
        state_after,
        state_unchanged: None,
        source_graph_unchanged: None,
        convergence_steps,
        live_matches_one_shot: None,
    }
}

struct LiveWatch {
    child: Child,
    lines: Receiver<String>,
    stdout_reader: Option<JoinHandle<Vec<u8>>>,
    stderr_reader: Option<JoinHandle<Vec<u8>>>,
    observed_lines: Vec<String>,
    stopped: bool,
}

impl LiveWatch {
    fn spawn(binary: &Path, root: &Path) -> Result<Self, String> {
        let mut child = Command::new(binary)
            .arg("watch")
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(display_error)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "watch stdout is missing".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "watch stderr is missing".to_string())?;
        let (sender, lines) = mpsc::channel();
        let stdout_reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            for line in BufReader::new(stdout).lines() {
                let line = line.unwrap_or_else(|error| format!("<stdout error: {error}>"));
                bytes.extend_from_slice(line.as_bytes());
                bytes.push(b'\n');
                if sender.send(line).is_err() {
                    break;
                }
            }
            bytes
        });
        let stderr_reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut stderr = stderr;
            let _ = stderr.read_to_end(&mut bytes);
            bytes
        });
        Ok(Self {
            child,
            lines,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(stderr_reader),
            observed_lines: Vec::new(),
            stopped: false,
        })
    }

    fn wait_until_ready(
        &mut self,
        timeout: Duration,
        started: Instant,
    ) -> Result<(f64, f64), String> {
        let deadline = Instant::now() + timeout;
        let mut publication = None;
        while Instant::now() < deadline {
            match self.lines.recv_timeout(Duration::from_millis(50)) {
                Ok(line) => {
                    if publication.is_none() && line.starts_with("state updated: snapshot ") {
                        publication = Some(started.elapsed().as_secs_f64());
                    }
                    let running = line == "criv watch running";
                    self.observed_lines.push(line);
                    if running {
                        return Ok((
                            publication.ok_or_else(|| {
                                "watch reached loop readiness before State publication".to_string()
                            })?,
                            started.elapsed().as_secs_f64(),
                        ));
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("watch output closed before readiness".into());
                }
            }
        }
        Err("timed out waiting for criv watch running".into())
    }

    fn wait_for_source_paths(
        &mut self,
        timeout: Duration,
        root: &Path,
        predicate: impl Fn(&BTreeSet<String>) -> bool,
    ) -> Result<StateIdentity, String> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            while let Ok(line) = self.lines.try_recv() {
                self.observed_lines.push(line);
            }
            if let Ok(observation) = read_state_observation(root) {
                let marker = format!(
                    "state updated: snapshot {},",
                    observation.identity.latest_snapshot
                );
                if predicate(&observation.source_paths)
                    && self
                        .observed_lines
                        .iter()
                        .any(|line| line.starts_with(&marker))
                {
                    return Ok(observation.identity);
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err("timed out waiting for live Source convergence".into())
    }

    fn stop(mut self) -> Result<ProcessOutput, String> {
        stop_child(&mut self.child)?;
        let output = reap_spawned_child(&mut self.child)?;
        self.stopped = true;
        let stdout = self
            .stdout_reader
            .take()
            .ok_or_else(|| "watch stdout reader is missing".to_string())?
            .join()
            .map_err(|_| "watch stdout reader panicked".to_string())?;
        let stderr = self
            .stderr_reader
            .take()
            .ok_or_else(|| "watch stderr reader is missing".to_string())?
            .join()
            .map_err(|_| "watch stderr reader panicked".to_string())?;
        Ok(ProcessOutput {
            status: output.status,
            stdout,
            stderr,
            user_seconds: output.user_seconds,
            system_seconds: output.system_seconds,
            peak_rss_bytes: output.peak_rss_bytes,
        })
    }
}

impl Drop for LiveWatch {
    fn drop(&mut self) {
        if !self.stopped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_state_identity(root: &Path) -> Result<StateIdentity, String> {
    read_state_observation(root).map(|observation| observation.identity)
}

fn read_state_observation(root: &Path) -> Result<StateObservation, String> {
    let criv_dir = root.join(".criv");
    for _ in 0..8 {
        let latest_snapshot = fs::read_to_string(criv_dir.join("latest"))
            .map_err(display_error)?
            .trim()
            .to_string();
        let snapshot_path = criv_dir.join(format!("snapshots/{latest_snapshot}.json"));
        let snapshot_bytes = fs::read(&snapshot_path).map_err(display_error)?;
        let state_bytes = fs::read(criv_dir.join("state.json")).map_err(display_error)?;
        let confirmed_latest = fs::read_to_string(criv_dir.join("latest"))
            .map_err(display_error)?
            .trim()
            .to_string();
        if confirmed_latest != latest_snapshot || state_bytes != snapshot_bytes {
            std::thread::yield_now();
            continue;
        }
        return state_observation(root, latest_snapshot, snapshot_bytes);
    }
    Err("State publication changed while it was read".into())
}

fn state_observation(
    root: &Path,
    latest_snapshot: String,
    state_bytes: Vec<u8>,
) -> Result<StateObservation, String> {
    let state: serde_json::Value = serde_json::from_slice(&state_bytes).map_err(display_error)?;
    let source_paths = state["source-index"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["path"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let mut vault_markdown = Vec::new();
    let mut vault_c4 = Vec::new();
    for node in state["graph"]["nodes"].as_array().into_iter().flatten() {
        let Some(path) = node["path"].as_str() else {
            continue;
        };
        match node["kind"].as_str() {
            Some("doc" | "decision") => vault_markdown.push(path.to_string()),
            Some("architecture-source") => vault_c4.push(path.to_string()),
            _ => {}
        }
    }
    let source_paths = sorted_unique(source_paths);
    let source_path_set = source_paths.iter().cloned().collect();
    let vault_markdown = sorted_unique(vault_markdown);
    let vault_c4 = sorted_unique(vault_c4);
    let identity = StateIdentity {
        state_digest: bytes_digest(&state_bytes),
        snapshot_digest: Some(bytes_digest(&state_bytes)),
        source_graph_digest: optional_file_digest(&root.join(".criv/source-graph.json")),
        source_paths: source_paths.len(),
        source_path_digest: path_digest("source", &source_paths),
        vault_markdown_paths: vault_markdown.len(),
        vault_markdown_path_digest: path_digest("vault-markdown", &vault_markdown),
        vault_c4_paths: vault_c4.len(),
        vault_c4_path_digest: path_digest("vault-c4", &vault_c4),
        latest_snapshot,
    };
    Ok(StateObservation {
        identity,
        source_paths: source_path_set,
    })
}

fn published_state_unchanged(before: &StateIdentity, after: &StateIdentity) -> bool {
    before.state_digest == after.state_digest
        && before.latest_snapshot == after.latest_snapshot
        && before.snapshot_digest == after.snapshot_digest
        && before.source_paths == after.source_paths
        && before.source_path_digest == after.source_path_digest
        && before.vault_markdown_paths == after.vault_markdown_paths
        && before.vault_markdown_path_digest == after.vault_markdown_path_digest
        && before.vault_c4_paths == after.vault_c4_paths
        && before.vault_c4_path_digest == after.vault_c4_path_digest
}

fn changed_markdown_reported_expected_diagnostic(
    output: &ProcessOutput,
    expected_path: &Path,
) -> bool {
    expected_markdown_diagnostic(
        output.status.code(),
        &output.stdout,
        &output.stderr,
        expected_path,
    )
}

fn full_check_completed(exit_status: Option<i32>, stdout: &[u8], stderr: &[u8]) -> bool {
    let Ok(diagnostics) = serde_json::from_slice::<Vec<serde_json::Value>>(stdout) else {
        return false;
    };
    match exit_status {
        Some(0) => stderr.is_empty(),
        Some(1) => {
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic["severity"] == "error")
                && String::from_utf8_lossy(stderr).contains("criv: check failed")
        }
        _ => false,
    }
}

fn expected_markdown_diagnostic(
    exit_status: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
    expected_path: &Path,
) -> bool {
    if exit_status != Some(1) || !String::from_utf8_lossy(stderr).contains("criv: check failed") {
        return false;
    }
    serde_json::from_slice::<Vec<serde_json::Value>>(stdout)
        .ok()
        .is_some_and(|diagnostics| {
            let expected = path_text(expected_path);
            diagnostics.iter().any(|diagnostic| {
                diagnostic["severity"] == "error"
                    && diagnostic["code"] == "markdown-format"
                    && diagnostic["path"] == expected
            })
        })
}

fn sorted_unique(mut paths: Vec<String>) -> Vec<String> {
    paths.sort();
    paths.dedup();
    paths
}

fn path_digest(domain: &str, paths: &[String]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain.as_bytes());
    for path in paths {
        hasher.update(&(path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn path_text(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn failed_outcome(error: String) -> CaseOutcome {
    CaseOutcome {
        successful: false,
        error: Some(error),
        exit_status: None,
        real_seconds: None,
        publication_ready_seconds: None,
        user_seconds: None,
        system_seconds: None,
        peak_rss_bytes: None,
        ready_rss_bytes: None,
        stdout: vec![],
        stderr: vec![],
        staged_patch_digest: None,
        state_before: None,
        state_after: None,
        state_unchanged: None,
        source_graph_unchanged: None,
        convergence_steps: vec![],
        live_matches_one_shot: None,
    }
}

fn summarize_attempt(case: Case, attempt: usize, rows: &[SampleRow]) -> AttemptSummary {
    let rows = rows.iter().collect::<Vec<_>>();
    summarize_attempt_refs(case, attempt, &rows)
}

fn summarize_attempt_refs(case: Case, attempt: usize, rows: &[&SampleRow]) -> AttemptSummary {
    let successful = rows
        .iter()
        .filter(|row| row.successful)
        .copied()
        .collect::<Vec<_>>();
    let real_seconds = metric(
        successful
            .iter()
            .filter_map(|row| row.real_seconds)
            .collect(),
    );
    let relative_mad = real_seconds.as_ref().and_then(|summary| {
        (summary.median > 0.0).then_some(summary.median_absolute_deviation / summary.median)
    });
    AttemptSummary {
        case: case.id().into(),
        attempt,
        successful_samples: successful.len(),
        failed_samples: rows.len() - successful.len(),
        real_seconds,
        publication_ready_seconds: metric(
            successful
                .iter()
                .filter_map(|row| row.publication_ready_seconds)
                .collect(),
        ),
        convergence_seconds: metric(
            successful
                .iter()
                .flat_map(|row| {
                    row.convergence_steps
                        .iter()
                        .map(|step| step.elapsed_seconds)
                })
                .collect(),
        ),
        peak_rss_bytes: metric(
            successful
                .iter()
                .filter_map(|row| row.peak_rss_bytes.map(|value| value as f64))
                .collect(),
        ),
        ready_rss_bytes: metric(
            successful
                .iter()
                .filter_map(|row| row.ready_rss_bytes.map(|value| value as f64))
                .collect(),
        ),
        relative_mad,
        unstable: relative_mad.is_some_and(|value| value > 0.10),
    }
}

fn ensure_stable_outputs(rows: &[SampleRow]) -> Result<(), String> {
    for case in ALL_CASES {
        let identities = rows
            .iter()
            .filter(|row| row.case == case.id() && row.successful)
            .map(stable_output_identity)
            .collect::<BTreeSet<_>>();
        if identities.len() > 1 {
            return Err(format!(
                "{} output identity changed between samples",
                case.id()
            ));
        }
    }
    Ok(())
}

fn stable_output_identity(row: &SampleRow) -> String {
    let identity = if row.case.starts_with("watch_") {
        let paths = row.state_after.as_ref().map(|state| {
            serde_json::json!({
                "source_paths": state.source_paths,
                "source_path_digest": state.source_path_digest,
                "vault_markdown_paths": state.vault_markdown_paths,
                "vault_markdown_path_digest": state.vault_markdown_path_digest,
                "vault_c4_paths": state.vault_c4_paths,
                "vault_c4_path_digest": state.vault_c4_path_digest,
            })
        });
        serde_json::json!({
            "exit_status": row.exit_status,
            "stderr_digest": row.stderr_digest,
            "paths": paths,
        })
    } else {
        serde_json::json!({
            "exit_status": row.exit_status,
            "stdout_digest": row.stdout_digest,
            "stderr_digest": row.stderr_digest,
            "state_unchanged": row.state_unchanged,
            "source_graph_unchanged": row.source_graph_unchanged,
        })
    };
    identity.to_string()
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

fn run_short(binary: &Path, root: &Path, args: &[&str]) -> Result<ProcessOutput, String> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_child(command)
}

#[cfg(unix)]
fn run_child(mut command: Command) -> Result<ProcessOutput, String> {
    let mut child = command.spawn().map_err(display_error)?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "child stdout pipe is missing".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "child stderr pipe is missing".to_string())?;
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.read_to_end(&mut bytes);
        (result, bytes)
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stderr.read_to_end(&mut bytes);
        (result, bytes)
    });
    let waited = reap_spawned_child(&mut child)?;
    let (stdout_result, stdout) = stdout_reader
        .join()
        .map_err(|_| "child stdout reader panicked".to_string())?;
    stdout_result.map_err(display_error)?;
    let (stderr_result, stderr) = stderr_reader
        .join()
        .map_err(|_| "child stderr reader panicked".to_string())?;
    stderr_result.map_err(display_error)?;
    Ok(ProcessOutput {
        status: waited.status,
        stdout,
        stderr,
        user_seconds: waited.user_seconds,
        system_seconds: waited.system_seconds,
        peak_rss_bytes: waited.peak_rss_bytes,
    })
}

#[cfg(not(unix))]
fn run_child(mut command: Command) -> Result<ProcessOutput, String> {
    let output = command.output().map_err(display_error)?;
    Ok(ProcessOutput {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
        user_seconds: None,
        system_seconds: None,
        peak_rss_bytes: None,
    })
}

struct WaitedChild {
    status: ExitStatus,
    user_seconds: Option<f64>,
    system_seconds: Option<f64>,
    peak_rss_bytes: Option<u64>,
}

#[cfg(unix)]
fn reap_spawned_child(child: &mut Child) -> Result<WaitedChild, String> {
    use std::os::unix::process::ExitStatusExt;

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
    Ok(WaitedChild {
        status: ExitStatus::from_raw(status),
        user_seconds: Some(timeval_seconds(usage.ru_utime)),
        system_seconds: Some(timeval_seconds(usage.ru_stime)),
        peak_rss_bytes: Some(peak_rss_bytes(&usage)),
    })
}

#[cfg(not(unix))]
fn reap_spawned_child(child: &mut Child) -> Result<WaitedChild, String> {
    Ok(WaitedChild {
        status: child.wait().map_err(display_error)?,
        user_seconds: None,
        system_seconds: None,
        peak_rss_bytes: None,
    })
}

#[cfg(unix)]
fn stop_child(child: &mut Child) -> Result<(), String> {
    // SAFETY: the pid belongs to the live child process.
    let status = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) };
    let error = std::io::Error::last_os_error();
    if status == 0 || error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(format!("failed to stop watch process: {error}"))
    }
}

#[cfg(not(unix))]
fn stop_child(child: &mut Child) -> Result<(), String> {
    child.kill().map_err(display_error)
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

#[cfg(target_os = "macos")]
fn current_rss_bytes(pid: u32) -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v0>::uninit();
    // SAFETY: usage is a valid output buffer for RUSAGE_INFO_V0.
    let status = unsafe {
        libc::proc_pid_rusage(
            pid as libc::c_int,
            libc::RUSAGE_INFO_V0,
            usage.as_mut_ptr().cast(),
        )
    };
    (status == 0).then(|| {
        // SAFETY: proc_pid_rusage initialized usage on a zero return code.
        unsafe { usage.assume_init() }.ri_resident_size
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn current_rss_bytes(pid: u32) -> Option<u64> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("VmRSS:")?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
            .map(|kib| kib.saturating_mul(1024))
    })
}

#[cfg(not(unix))]
fn current_rss_bytes(_pid: u32) -> Option<u64> {
    None
}

fn canonical_file(path: &Path) -> Result<PathBuf, String> {
    let path = fs::canonicalize(path).map_err(display_error)?;
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("not a file: {}", path.display()))
    }
}

fn canonical_directory(path: &Path) -> Result<PathBuf, String> {
    let path = fs::canonicalize(path).map_err(display_error)?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!("not a directory: {}", path.display()))
    }
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

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<(), String> {
    let mut output = BufWriter::new(File::create(path).map_err(display_error)?);
    serde_json::to_writer_pretty(&mut output, value).map_err(display_error)?;
    output.write_all(b"\n").map_err(display_error)
}

fn command_bytes(root: &Path, program: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(display_error)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn command_text(root: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    String::from_utf8(command_bytes(root, program, args)?)
        .map(|value| value.trim().to_string())
        .map_err(display_error)
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

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_paths_must_be_normalized_and_relative() {
        assert!(validate_relative_path(Path::new("src/file.rs")).is_ok());
        assert!(validate_relative_path(Path::new("../file.rs")).is_err());
        assert!(validate_relative_path(Path::new("src/../file.rs")).is_err());
    }

    #[test]
    fn snapshot_restore_resets_tracked_and_generated_sample_state() {
        let root = tempfile::TempDir::new().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/file.rs"), "original\n").unwrap();
        for command in [
            &["init", "--quiet"][..],
            &["config", "user.email", "performance@criv.invalid"][..],
            &["config", "user.name", "criv performance"][..],
            &["add", "--all"][..],
            &["commit", "--quiet", "-m", "fixture"][..],
        ] {
            assert!(
                Command::new("git")
                    .args(command)
                    .current_dir(root.path())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let original = fs::read(root.path().join("src/file.rs")).unwrap();
        fs::write(root.path().join("src/file.rs"), "changed\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "--all"])
                .current_dir(root.path())
                .status()
                .unwrap()
                .success()
        );
        fs::create_dir(root.path().join(".criv")).unwrap();
        fs::write(root.path().join(".criv/state.json"), "state\n").unwrap();
        fs::write(
            root.path().join("src/criv_discovery_watch_probe.rs"),
            "probe\n",
        )
        .unwrap();

        let args = Args {
            binary: "criv".into(),
            binary_label: "test".into(),
            snapshot_executable: "snapshot".into(),
            workload_root: root.path().into(),
            workload_inventory: "inventory.json".into(),
            sample_root: root.path().into(),
            results_root: root.path().into(),
            source_mutation_path: None,
            markdown_mutation_path: None,
            live_mutation_directory: Some("src".into()),
            cases: vec![],
            samples: 5,
            allow_low_samples: false,
            timeout_seconds: 120,
            minimum_free_gib: 30,
            maximum_snapshot_allocation_gib: 20,
        };
        restore_snapshot(&args, root.path()).unwrap();

        assert_eq!(fs::read(root.path().join("src/file.rs")).unwrap(), original);
        assert!(!root.path().join(".criv").exists());
        assert!(
            !root
                .path()
                .join("src/criv_discovery_watch_probe.rs")
                .exists()
        );
        assert!(
            command_bytes(root.path(), "git", &["diff", "--cached"])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn state_identity_separates_profile_paths() {
        let root = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(root.path().join(".criv/snapshots")).unwrap();
        let state = serde_json::json!({
            "source-index": [{"path": "src/lib.rs"}],
            "graph": {"nodes": [
                {"kind": "doc", "path": "docs/readme.md"},
                {"kind": "decision", "path": "docs/adr/0001.md"},
                {"kind": "architecture-source", "path": "docs/model.c4"}
            ]}
        });
        let bytes = serde_json::to_vec(&state).unwrap();
        let snapshot = blake3::hash(&bytes).to_hex().to_string();
        fs::write(root.path().join(".criv/state.json"), &bytes).unwrap();
        fs::write(root.path().join(".criv/latest"), &snapshot).unwrap();
        fs::write(
            root.path().join(format!(".criv/snapshots/{snapshot}.json")),
            &bytes,
        )
        .unwrap();

        let observation = read_state_observation(root.path()).unwrap();
        let identity = observation.identity;
        assert_eq!(identity.source_paths, 1);
        assert!(observation.source_paths.contains("src/lib.rs"));
        assert_eq!(identity.vault_markdown_paths, 2);
        assert_eq!(identity.vault_c4_paths, 1);
        assert_ne!(
            identity.source_path_digest,
            identity.vault_markdown_path_digest
        );
    }

    #[test]
    fn published_state_ignores_source_graph_cache_changes() {
        let before = StateIdentity {
            state_digest: "state".into(),
            latest_snapshot: "snapshot".into(),
            snapshot_digest: Some("snapshot-digest".into()),
            source_graph_digest: Some("old-cache".into()),
            source_paths: 1,
            source_path_digest: "source".into(),
            vault_markdown_paths: 1,
            vault_markdown_path_digest: "markdown".into(),
            vault_c4_paths: 1,
            vault_c4_path_digest: "c4".into(),
        };
        let mut after = before.clone();
        after.source_graph_digest = Some("new-cache".into());

        assert!(published_state_unchanged(&before, &after));
        assert_ne!(before.source_graph_digest, after.source_graph_digest);
    }

    #[test]
    fn expected_markdown_diagnostic_requires_the_mutated_file() {
        let stdout = br#"[{"severity":"error","code":"markdown-format","path":"content/file.md"}]"#;
        assert!(expected_markdown_diagnostic(
            Some(1),
            stdout,
            b"criv: check failed\n",
            Path::new("content/file.md")
        ));
        assert!(!expected_markdown_diagnostic(
            Some(1),
            stdout,
            b"criv: check failed\n",
            Path::new("content/other.md")
        ));
    }

    #[test]
    fn full_check_accepts_clean_and_diagnostic_results() {
        assert!(full_check_completed(Some(0), b"[]\n", b""));
        assert!(full_check_completed(
            Some(1),
            br#"[{"severity":"error","code":"markdown-format"}]"#,
            b"criv: check failed\n"
        ));
    }

    #[test]
    fn watch_output_stability_uses_selected_path_identity() {
        let mut first = test_row(Case::WatchOnceCold, 1.0);
        first.state_after = Some(test_state_identity("state-a", "source"));
        let mut second = test_row(Case::WatchOnceCold, 1.0);
        second.stdout_digest = "different-snapshot-line".into();
        second.state_after = Some(test_state_identity("state-b", "source"));

        assert!(ensure_stable_outputs(&[first, second]).is_ok());

        let mut changed = test_row(Case::WatchOnceCold, 1.0);
        changed.state_after = Some(test_state_identity("state-c", "changed-source"));
        assert!(
            ensure_stable_outputs(&[test_row_with_state("state-a", "source"), changed,]).is_err()
        );
    }

    #[test]
    fn unstable_attempt_uses_relative_mad() {
        let rows = [0.8, 0.9, 1.0, 1.2, 1.4]
            .into_iter()
            .map(|real_seconds| test_row(Case::CheckFull, real_seconds))
            .collect::<Vec<_>>();
        assert!(summarize_attempt(Case::CheckFull, 1, &rows).unstable);
    }

    fn test_row(case: Case, real_seconds: f64) -> SampleRow {
        SampleRow {
            schema: SAMPLE_SCHEMA,
            run_id: "run".into(),
            workload_id: "workload".into(),
            workload_digest: "a".repeat(64),
            binary_label: "binary".into(),
            binary_digest: "b".repeat(64),
            case: case.id().into(),
            cache_state: "cold".into(),
            attempt: 1,
            sample: 1,
            successful: true,
            error: None,
            exit_status: Some(0),
            real_seconds: Some(real_seconds),
            publication_ready_seconds: None,
            user_seconds: None,
            system_seconds: None,
            peak_rss_bytes: Some(1),
            ready_rss_bytes: None,
            stdout_digest: "stdout".into(),
            stderr_digest: "stderr".into(),
            stdout_path: String::new(),
            stderr_path: String::new(),
            snapshot_receipt_digest: "snapshot".into(),
            staged_patch_digest: None,
            state_before: None,
            state_after: None,
            state_unchanged: None,
            source_graph_unchanged: None,
            convergence_steps: vec![],
            live_matches_one_shot: None,
        }
    }

    fn test_row_with_state(state_digest: &str, source_digest: &str) -> SampleRow {
        let mut row = test_row(Case::WatchOnceCold, 1.0);
        row.state_after = Some(test_state_identity(state_digest, source_digest));
        row
    }

    fn test_state_identity(state_digest: &str, source_digest: &str) -> StateIdentity {
        StateIdentity {
            state_digest: state_digest.into(),
            latest_snapshot: state_digest.into(),
            snapshot_digest: Some(state_digest.into()),
            source_graph_digest: Some("source-graph".into()),
            source_paths: 1,
            source_path_digest: source_digest.into(),
            vault_markdown_paths: 1,
            vault_markdown_path_digest: "markdown".into(),
            vault_c4_paths: 1,
            vault_c4_path_digest: "c4".into(),
        }
    }
}
