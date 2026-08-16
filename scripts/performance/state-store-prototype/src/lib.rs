use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use clap::{Parser, ValueEnum};
use criv_state_wire::STATE_SCHEMA;
use serde::{Deserialize, Serialize};

#[path = "generated/state_store_generated.rs"]
#[allow(clippy::derivable_impls, clippy::extra_unused_lifetimes)]
mod state_store_generated;
use state_store_generated::criv::bench as fb;

const REPORT_SCHEMA: &str = "criv.state-store-candidate.v1";

#[derive(Debug, Parser)]
#[command(
    name = "criv-state-store-bench",
    about = "PROTOTYPE: compare machine State store candidates"
)]
struct Args {
    #[arg(long, value_enum)]
    candidate: Candidate,
    #[arg(long)]
    state: Option<PathBuf>,
    #[arg(long)]
    changed_state: Option<PathBuf>,
    #[arg(long = "snapshot", num_args = 1..)]
    snapshots: Vec<PathBuf>,
    #[arg(long)]
    wasm_package: Option<PathBuf>,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long, default_value_t = 5)]
    samples: usize,
    #[arg(long)]
    allow_low_samples: bool,
    #[arg(long, hide = true)]
    worker_operation: Option<String>,
    #[arg(long, hide = true)]
    store: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
enum Candidate {
    CrivColumn,
    Flatbuffers,
    Json,
    Postcard,
    Rkyv,
}

impl Candidate {
    fn id(self) -> &'static str {
        match self {
            Self::CrivColumn => "criv-column",
            Self::Flatbuffers => "flatbuffers",
            Self::Json => "json",
            Self::Postcard => "postcard",
            Self::Rkyv => "rkyv",
        }
    }

    fn layout(self) -> &'static str {
        match self {
            Self::CrivColumn => "partitioned-column-store",
            Self::Flatbuffers => "partitioned-flatbuffers",
            Self::Json => "json-baseline",
            Self::Postcard => "full-decode-control",
            Self::Rkyv => "partitioned-checked-archive-upper-bound",
        }
    }

    fn partition_magic(self) -> Option<[u8; 4]> {
        match self {
            Self::CrivColumn => Some(*b"CRCL"),
            Self::Flatbuffers => Some(*b"CRFB"),
            Self::Rkyv => Some(*b"CRRK"),
            Self::Json | Self::Postcard => None,
        }
    }
}

#[derive(Debug, Serialize)]
struct Correctness {
    deterministic_bytes: bool,
    logical_round_trip: bool,
    rejects_truncated: bool,
    rejects_corrupt: bool,
    rejects_unknown_version: bool,
    interrupted_publication_keeps_current: bool,
}

#[derive(Debug, Serialize)]
struct Report {
    schema: &'static str,
    candidate: Candidate,
    layout: &'static str,
    samples: usize,
    evidence: Evidence,
    storage: Storage,
    native: NativeReport,
    wasm: Option<serde_json::Value>,
    correctness: Correctness,
}

#[derive(Debug, Serialize)]
struct Evidence {
    profile: &'static str,
    operating_system: &'static str,
    architecture: &'static str,
    state: String,
    state_digest: String,
    changed_state: String,
    changed_state_digest: String,
    snapshot_count: usize,
    benchmark_binary_digest: String,
    architecture_payload_present: bool,
}

#[derive(Debug, Serialize)]
struct Storage {
    stored_bytes: usize,
    retained_snapshot_bytes: usize,
    changed_publication_bytes: usize,
    partition_count: usize,
    changed_partition_count: usize,
    reused_partition_count: usize,
    edge_endpoint_width_bits: u8,
    publication_model: &'static str,
}

#[derive(Debug, Serialize)]
struct NativeReport {
    operations: BTreeMap<String, Timing>,
    evidence: NativeEvidence,
    load_validate_peak_rss_bytes: Option<u64>,
    measurement_process_peak_rss_bytes: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
struct NativeWorkerSample {
    seconds: f64,
    peak_rss_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
struct Timing {
    samples: usize,
    raw_seconds: Vec<f64>,
    minimum_seconds: f64,
    median_seconds: f64,
    maximum_seconds: f64,
    median_absolute_deviation_seconds: f64,
}

#[derive(Debug, Serialize)]
struct NativeEvidence {
    node_count: usize,
    edge_count: usize,
    source_count: usize,
    lookup_present: bool,
    lookup_missing: bool,
    exact_selector_target: Option<String>,
    nodes_added: usize,
    nodes_changed: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct JsonState {
    schema: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    architecture: Option<serde_json::Value>,
    graph: Graph,
    #[serde(default, rename = "registered-patterns")]
    registered_patterns: Vec<String>,
    #[serde(default)]
    patterns: BTreeMap<String, Vec<PatternMatch>>,
    #[serde(default, rename = "source-index")]
    source_index: Vec<SourceIndexEntry>,
}

#[derive(
    Debug,
    Clone,
    Deserialize,
    PartialEq,
    Serialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
struct MachineState {
    schema: String,
    architecture_json: Option<Vec<u8>>,
    graph: Graph,
    registered_patterns: Vec<String>,
    patterns: BTreeMap<String, Vec<PatternMatch>>,
    source_index: Vec<SourceIndexEntry>,
}

#[derive(
    Debug,
    Clone,
    Deserialize,
    PartialEq,
    Serialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
struct Graph {
    root: String,
    #[serde(default)]
    nodes: Vec<Node>,
    #[serde(default)]
    edges: Vec<Edge>,
}

#[derive(
    Debug,
    Clone,
    Deserialize,
    PartialEq,
    Serialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
struct Node {
    id: String,
    hash: String,
    kind: String,
    label: String,
    path: Option<String>,
}

#[derive(
    Debug,
    Clone,
    Deserialize,
    PartialEq,
    Serialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
struct Edge {
    from: String,
    to: String,
    kind: String,
    hash: String,
}

#[derive(
    Debug,
    Clone,
    Deserialize,
    PartialEq,
    Serialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
struct PatternMatch {
    file: String,
    range: Option<String>,
    #[serde(default)]
    captures: BTreeMap<String, String>,
}

#[derive(
    Debug,
    Clone,
    Deserialize,
    PartialEq,
    Serialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
struct SourceIndexEntry {
    path: String,
    mime: Option<String>,
}

impl MachineState {
    fn from_value(value: &serde_json::Value) -> Result<Self, String> {
        let state = serde_json::from_value::<JsonState>(value.clone())
            .map_err(|error| format!("invalid logical State: {error}"))?;
        if state.schema != STATE_SCHEMA {
            return Err(format!("unsupported State schema: {}", state.schema));
        }
        let architecture_json = state
            .architecture
            .map(|architecture| {
                serde_json::to_vec(&architecture)
                    .map_err(|error| format!("failed to encode architecture data: {error}"))
            })
            .transpose()?;
        Ok(Self {
            schema: state.schema,
            architecture_json,
            graph: state.graph,
            registered_patterns: state.registered_patterns,
            patterns: state.patterns,
            source_index: state.source_index,
        })
    }

    fn to_value(&self) -> Result<serde_json::Value, String> {
        let architecture = self
            .architecture_json
            .as_deref()
            .map(|bytes| {
                serde_json::from_slice(bytes)
                    .map_err(|error| format!("invalid stored architecture data: {error}"))
            })
            .transpose()?;
        serde_json::to_value(JsonState {
            schema: self.schema.clone(),
            architecture,
            graph: self.graph.clone(),
            registered_patterns: self.registered_patterns.clone(),
            patterns: self.patterns.clone(),
            source_index: self.source_index.clone(),
        })
        .map_err(|error| format!("failed to restore logical State: {error}"))
    }
}

pub fn main_entry() {
    if let Err(error) = run() {
        eprintln!("criv-state-store-bench: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse();
    if args.worker_operation.is_some() {
        return run_worker(&args);
    }
    if args.samples == 0 || (args.samples < 3 && !args.allow_low_samples) {
        return Err("samples must be at least 3; use --allow-low-samples only in tests".into());
    }
    let current_path = args
        .state
        .as_deref()
        .ok_or_else(|| "--state is required".to_string())?;
    let changed_path = args
        .changed_state
        .as_deref()
        .ok_or_else(|| "--changed-state is required".to_string())?;
    let current = read_state(current_path)?;
    let changed = read_state(changed_path)?;
    let snapshot_states = if args.snapshots.is_empty() {
        vec![current.clone(), changed.clone()]
    } else {
        args.snapshots
            .iter()
            .map(|path| read_state(path))
            .collect::<Result<Vec<_>, _>>()?
    };
    let first = encode_candidate(args.candidate, &current)?;
    let second = encode_candidate(args.candidate, &current)?;
    let decoded = decode_candidate(args.candidate, &first)?;
    let changed_bytes = encode_candidate(args.candidate, &changed)?;
    let encoded_snapshots = snapshot_states
        .iter()
        .map(|state| encode_candidate(args.candidate, state))
        .collect::<Result<Vec<_>, _>>()?;
    let wasm = args
        .wasm_package
        .as_deref()
        .map(|package| {
            measure_wasm(
                args.candidate,
                &first,
                package,
                args.samples,
                args.allow_low_samples,
            )
        })
        .transpose()?;

    let truncated = &first[..first.len() / 2];
    let corrupt = corrupt_bytes(args.candidate, &first);
    let unknown = unknown_version_bytes(args.candidate, &current)?;

    let report = Report {
        schema: REPORT_SCHEMA,
        candidate: args.candidate,
        layout: args.candidate.layout(),
        samples: args.samples,
        evidence: Evidence {
            profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            operating_system: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            state: current_path.display().to_string(),
            state_digest: file_digest(current_path)?,
            changed_state: changed_path.display().to_string(),
            changed_state_digest: file_digest(changed_path)?,
            snapshot_count: snapshot_states.len(),
            benchmark_binary_digest: file_digest(
                &std::env::current_exe()
                    .map_err(|error| format!("failed to resolve benchmark executable: {error}"))?,
            )?,
            architecture_payload_present: current.get("architecture").is_some(),
        },
        storage: storage_report(args.candidate, &first, &changed_bytes, &encoded_snapshots)?,
        native: measure_native(
            args.candidate,
            &current,
            &changed,
            &snapshot_states,
            args.samples,
        )?,
        wasm,
        correctness: Correctness {
            deterministic_bytes: first == second,
            logical_round_trip: decoded == current
                && decode_candidate(args.candidate, &changed_bytes)? == changed,
            rejects_truncated: decode_candidate(args.candidate, truncated).is_err(),
            rejects_corrupt: decode_candidate(args.candidate, &corrupt).is_err(),
            rejects_unknown_version: decode_candidate(args.candidate, &unknown).is_err(),
            interrupted_publication_keeps_current: interrupted_publication_keeps_current(
                args.candidate,
                &first,
                &changed_bytes,
            )?,
        },
    };
    let report = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("failed to encode report: {error}"))?;
    if let Some(path) = &args.output {
        fs::write(path, report)
            .map_err(|error| format!("failed to write report {}: {error}", path.display()))?;
    } else {
        println!("{}", String::from_utf8(report).unwrap());
    }
    Ok(())
}

fn run_worker(args: &Args) -> Result<(), String> {
    if args.worker_operation.as_deref() != Some("load-validate") {
        return Err(format!(
            "unsupported worker operation: {}",
            args.worker_operation.as_deref().unwrap_or("<missing>")
        ));
    }
    let path = args
        .store
        .as_deref()
        .ok_or_else(|| "--store is required for a worker operation".to_string())?;
    let start = Instant::now();
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read worker State {}: {error}", path.display()))?;
    std::hint::black_box(decode_machine_candidate(args.candidate, &bytes)?);
    let sample = NativeWorkerSample {
        seconds: start.elapsed().as_secs_f64(),
        peak_rss_bytes: peak_rss_bytes(),
    };
    println!(
        "{}",
        serde_json::to_string(&sample)
            .map_err(|error| format!("failed to encode worker sample: {error}"))?
    );
    Ok(())
}

fn measure_wasm(
    candidate: Candidate,
    bytes: &[u8],
    package: &Path,
    samples: usize,
    allow_low_samples: bool,
) -> Result<serde_json::Value, String> {
    let root = tempfile::tempdir()
        .map_err(|error| format!("failed to create Wasm sample directory: {error}"))?;
    let store = root.path().join("state.bin");
    fs::write(&store, bytes)
        .map_err(|error| format!("failed to write Wasm candidate State: {error}"))?;
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../measure-state-candidate-wasm.mjs");
    let mut command = Command::new("node");
    command
        .arg(script)
        .args(["--candidate", candidate.id()])
        .arg("--store")
        .arg(&store)
        .arg("--package")
        .arg(package)
        .args(["--samples", &samples.to_string()]);
    if allow_low_samples {
        command.arg("--allow-low-samples");
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to start candidate Wasm measurement: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "candidate Wasm measurement failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid candidate Wasm report: {error}"))
}

fn read_state(path: &Path) -> Result<serde_json::Value, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read State {}: {error}", path.display()))?;
    decode_json(&bytes)
}

fn file_digest(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read {} for identity: {error}", path.display()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn encode_json(state: &serde_json::Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec(state).map_err(|error| format!("failed to encode JSON State: {error}"))
}

fn encode_candidate(candidate: Candidate, state: &serde_json::Value) -> Result<Vec<u8>, String> {
    match candidate {
        Candidate::CrivColumn => encode_column_store(&MachineState::from_value(state)?),
        Candidate::Flatbuffers => encode_flatbuffers_store(&MachineState::from_value(state)?),
        Candidate::Json => encode_json(state),
        Candidate::Postcard => {
            let state = MachineState::from_value(state)?;
            let payload = postcard::to_allocvec(&state)
                .map_err(|error| format!("failed to encode Postcard State: {error}"))?;
            Ok(envelope(*b"CRPC", 1, &payload))
        }
        Candidate::Rkyv => encode_rkyv_store(&MachineState::from_value(state)?),
    }
}

fn decode_candidate(candidate: Candidate, bytes: &[u8]) -> Result<serde_json::Value, String> {
    decode_machine_candidate(candidate, bytes)?.to_value()
}

fn decode_machine_candidate(candidate: Candidate, bytes: &[u8]) -> Result<MachineState, String> {
    match candidate {
        #[cfg(any(feature = "wasm-all", feature = "wasm-criv-column"))]
        Candidate::CrivColumn => decode_column_store(bytes),
        #[cfg(any(feature = "wasm-all", feature = "wasm-flatbuffers"))]
        Candidate::Flatbuffers => decode_flatbuffers_store(bytes),
        #[cfg(any(feature = "wasm-all", feature = "wasm-json"))]
        Candidate::Json => MachineState::from_value(&decode_json(bytes)?),
        #[cfg(any(feature = "wasm-all", feature = "wasm-postcard"))]
        Candidate::Postcard => {
            let payload = open_envelope(bytes, *b"CRPC", 1)?;
            let state = postcard::from_bytes::<MachineState>(payload)
                .map_err(|error| format!("invalid Postcard State: {error}"))?;
            if state.schema != STATE_SCHEMA {
                return Err(format!("unsupported State schema: {}", state.schema));
            }
            Ok(state)
        }
        #[cfg(any(feature = "wasm-all", feature = "wasm-rkyv"))]
        Candidate::Rkyv => decode_rkyv_store(bytes),
        #[allow(unreachable_patterns)]
        _ => Err(format!(
            "candidate {} is not present in this package",
            candidate.id()
        )),
    }
}

fn measure_native(
    candidate: Candidate,
    current: &serde_json::Value,
    changed: &serde_json::Value,
    snapshots: &[serde_json::Value],
    samples: usize,
) -> Result<NativeReport, String> {
    let current_bytes = encode_candidate(candidate, current)?;
    let changed_bytes = encode_candidate(candidate, changed)?;
    let snapshot_bytes = snapshots
        .iter()
        .map(|state| encode_candidate(candidate, state))
        .collect::<Result<Vec<_>, _>>()?;
    let current_state = decode_machine_candidate(candidate, &current_bytes)?;
    let changed_state = decode_machine_candidate(candidate, &changed_bytes)?;
    let prepared = PreparedState::new(current_state.clone());
    let present = current_state
        .graph
        .nodes
        .first()
        .map(|node| node.id.as_str())
        .unwrap_or("__criv_missing_node__");
    let exact = prepared
        .sources
        .first()
        .map(|source| source.path.as_str())
        .unwrap_or("__criv_missing_source__");
    let suffix = exact.rsplit('/').next().unwrap_or(exact);
    let diff = diff_states(&current_state, &changed_state);
    let mut operations = BTreeMap::new();

    operations.insert(
        "complete_publication".into(),
        measure(samples, || {
            let root = tempfile::tempdir()
                .map_err(|error| format!("failed to create publication sample: {error}"))?;
            let start = Instant::now();
            let bytes = encode_candidate(candidate, current)?;
            publish_encoded(candidate, root.path(), &bytes, "latest", "snapshot")?;
            Ok(start.elapsed())
        })?,
    );
    operations.insert(
        "changed_publication".into(),
        measure(samples, || {
            let root = tempfile::tempdir()
                .map_err(|error| format!("failed to create changed sample: {error}"))?;
            seed_encoded(candidate, root.path(), &current_bytes)?;
            let start = Instant::now();
            let bytes = encode_candidate(candidate, changed)?;
            publish_encoded(candidate, root.path(), &bytes, "latest", "snapshot")?;
            Ok(start.elapsed())
        })?,
    );
    let (load_validate, load_validate_peak_rss_bytes) =
        measure_file_load(samples, candidate, &current_bytes)?;
    operations.insert("load_validate".into(), load_validate);
    operations.insert(
        "lookup_present".into(),
        measure(samples, || {
            let start = Instant::now();
            std::hint::black_box(prepared.lookup(present));
            Ok(start.elapsed())
        })?,
    );
    operations.insert(
        "lookup_missing".into(),
        measure(samples, || {
            let start = Instant::now();
            std::hint::black_box(prepared.lookup("__criv_missing_node__"));
            Ok(start.elapsed())
        })?,
    );
    for (name, query) in [
        ("selector_empty", ""),
        ("selector_exact", exact),
        ("selector_suffix", suffix),
        ("selector_missing", "__criv_missing_selector__"),
    ] {
        operations.insert(
            name.into(),
            measure(samples, || {
                let start = Instant::now();
                std::hint::black_box(prepared.selectors(query, 20));
                Ok(start.elapsed())
            })?,
        );
    }
    operations.insert(
        "two_state_diff".into(),
        measure_two_state_diff(samples, candidate, &current_bytes, &changed_bytes)?,
    );
    operations.insert(
        "snapshot_list".into(),
        measure_snapshot_list(samples, candidate, &snapshot_bytes)?,
    );

    Ok(NativeReport {
        evidence: NativeEvidence {
            node_count: current_state.graph.nodes.len(),
            edge_count: current_state.graph.edges.len(),
            source_count: prepared.sources.len(),
            lookup_present: prepared.lookup(present),
            lookup_missing: prepared.lookup("__criv_missing_node__"),
            exact_selector_target: prepared.selectors(exact, 20).into_iter().next(),
            nodes_added: diff.0,
            nodes_changed: diff.1,
        },
        operations,
        load_validate_peak_rss_bytes,
        measurement_process_peak_rss_bytes: peak_rss_bytes(),
    })
}

fn measure(
    samples: usize,
    mut operation: impl FnMut() -> Result<std::time::Duration, String>,
) -> Result<Timing, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        values.push(operation()?.as_secs_f64());
    }
    Ok(timing(values))
}

fn timing(mut values: Vec<f64>) -> Timing {
    let raw_seconds = values.clone();
    values.sort_by(f64::total_cmp);
    let center = median(&values);
    let mut deviations = values
        .iter()
        .map(|value| (value - center).abs())
        .collect::<Vec<_>>();
    deviations.sort_by(f64::total_cmp);
    Timing {
        samples: values.len(),
        raw_seconds,
        minimum_seconds: values[0],
        median_seconds: center,
        maximum_seconds: values[values.len() - 1],
        median_absolute_deviation_seconds: median(&deviations),
    }
}

fn median(values: &[f64]) -> f64 {
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

fn measure_file_load(
    samples: usize,
    candidate: Candidate,
    bytes: &[u8],
) -> Result<(Timing, Option<u64>), String> {
    let root =
        tempfile::tempdir().map_err(|error| format!("failed to create load sample: {error}"))?;
    let path = root.path().join("state.bin");
    fs::write(&path, bytes).map_err(|error| format!("failed to seed load State: {error}"))?;
    run_native_load_worker(candidate, &path)?;
    let mut timings = Vec::with_capacity(samples);
    let mut memory = Vec::with_capacity(samples);
    for _ in 0..samples {
        let sample = run_native_load_worker(candidate, &path)?;
        timings.push(sample.seconds);
        if let Some(value) = sample.peak_rss_bytes {
            memory.push(value);
        }
    }
    memory.sort_unstable();
    let peak = (memory.len() == samples).then(|| memory[memory.len() / 2]);
    Ok((timing(timings), peak))
}

fn run_native_load_worker(candidate: Candidate, path: &Path) -> Result<NativeWorkerSample, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("failed to resolve candidate executable: {error}"))?;
    let output = Command::new(executable)
        .args([
            "--candidate",
            candidate.id(),
            "--worker-operation",
            "load-validate",
        ])
        .arg("--store")
        .arg(path)
        .output()
        .map_err(|error| format!("failed to start native load worker: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "native load worker failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid native load worker sample: {error}"))
}

fn measure_two_state_diff(
    samples: usize,
    candidate: Candidate,
    current: &[u8],
    changed: &[u8],
) -> Result<Timing, String> {
    let root =
        tempfile::tempdir().map_err(|error| format!("failed to create diff sample: {error}"))?;
    let current_path = root.path().join("current.bin");
    let changed_path = root.path().join("changed.bin");
    fs::write(&current_path, current).map_err(|error| format!("failed to seed diff: {error}"))?;
    fs::write(&changed_path, changed).map_err(|error| format!("failed to seed diff: {error}"))?;
    measure(samples, || {
        let start = Instant::now();
        let current = decode_machine_candidate(
            candidate,
            &fs::read(&current_path)
                .map_err(|error| format!("failed to read diff State: {error}"))?,
        )?;
        let changed = decode_machine_candidate(
            candidate,
            &fs::read(&changed_path)
                .map_err(|error| format!("failed to read diff State: {error}"))?,
        )?;
        std::hint::black_box(diff_states(&current, &changed));
        Ok(start.elapsed())
    })
}

fn measure_snapshot_list(
    samples: usize,
    candidate: Candidate,
    snapshots: &[Vec<u8>],
) -> Result<Timing, String> {
    let root = tempfile::tempdir()
        .map_err(|error| format!("failed to create snapshot sample: {error}"))?;
    let snapshot_root = root.path().join("snapshots");
    fs::create_dir(&snapshot_root)
        .map_err(|error| format!("failed to create snapshot list: {error}"))?;
    if let Some(magic) = candidate.partition_magic() {
        for (index, bytes) in snapshots.iter().enumerate() {
            let partitions = open_partition_package(bytes, magic)?;
            write_partition_objects(root.path(), &partitions)?;
            fs::write(
                snapshot_root.join(format!("{index:04}.manifest")),
                encode_partition_manifest(magic, &partitions),
            )
            .map_err(|error| format!("failed to seed snapshot manifest: {error}"))?;
        }
    } else {
        for (index, bytes) in snapshots.iter().enumerate() {
            fs::write(snapshot_root.join(format!("{index:04}.bin")), bytes)
                .map_err(|error| format!("failed to seed snapshot: {error}"))?;
        }
    }
    measure(samples, || {
        let start = Instant::now();
        let mut rows = fs::read_dir(&snapshot_root)
            .map_err(|error| format!("failed to list snapshots: {error}"))?
            .map(|entry| {
                let entry =
                    entry.map_err(|error| format!("failed to read snapshot row: {error}"))?;
                let len = entry
                    .metadata()
                    .map_err(|error| format!("failed to read snapshot metadata: {error}"))?
                    .len();
                Ok((entry.file_name(), len))
            })
            .collect::<Result<Vec<_>, String>>()?;
        rows.sort();
        std::hint::black_box(rows);
        Ok(start.elapsed())
    })
}

fn seed_encoded(candidate: Candidate, root: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(magic) = candidate.partition_magic() {
        let partitions = open_partition_package(bytes, magic)?;
        write_partition_objects(root, &partitions)
    } else {
        fs::write(root.join("current.bin"), bytes)
            .map_err(|error| format!("failed to seed current State: {error}"))
    }
}

fn publish_encoded(
    candidate: Candidate,
    root: &Path,
    bytes: &[u8],
    latest_name: &str,
    snapshot_name: &str,
) -> Result<(), String> {
    if let Some(magic) = candidate.partition_magic() {
        let partitions = open_partition_package(bytes, magic)?;
        write_partition_objects(root, &partitions)?;
        let manifest = encode_partition_manifest(magic, &partitions);
        fs::write(root.join(format!("{latest_name}.manifest")), &manifest)
            .map_err(|error| format!("failed to write latest manifest: {error}"))?;
        fs::write(root.join(format!("{snapshot_name}.manifest")), &manifest)
            .map_err(|error| format!("failed to write snapshot manifest: {error}"))?;
        Ok(())
    } else {
        fs::write(root.join(format!("{latest_name}.bin")), bytes)
            .map_err(|error| format!("failed to write latest State: {error}"))?;
        fs::write(root.join(format!("{snapshot_name}.bin")), bytes)
            .map_err(|error| format!("failed to write State snapshot: {error}"))
    }
}

fn write_partition_objects(root: &Path, partitions: &[StoredPartition]) -> Result<(), String> {
    let objects = root.join("objects");
    fs::create_dir_all(&objects)
        .map_err(|error| format!("failed to create partition pool: {error}"))?;
    for partition in partitions {
        let name = partition
            .hash
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let path = objects.join(name);
        if !path.exists() {
            fs::write(path, &partition.payload)
                .map_err(|error| format!("failed to write State partition: {error}"))?;
        }
    }
    Ok(())
}

fn encode_partition_manifest(magic: [u8; 4], partitions: &[StoredPartition]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(8 + partitions.len() * 45);
    payload.extend_from_slice(&magic);
    payload.extend_from_slice(&(partitions.len() as u32).to_le_bytes());
    for partition in partitions {
        payload.push(partition.kind);
        payload.extend_from_slice(&partition.block.to_le_bytes());
        payload.extend_from_slice(&(partition.payload.len() as u64).to_le_bytes());
        payload.extend_from_slice(&partition.hash);
    }
    envelope(*b"CRMF", 1, &payload)
}

#[derive(Debug)]
struct PreparedState {
    node_targets: std::collections::BTreeSet<String>,
    sources: Vec<SourceIndexEntry>,
    selector_targets: Vec<String>,
}

impl PreparedState {
    fn new(state: MachineState) -> Self {
        let mut node_targets = std::collections::BTreeSet::new();
        let mut selector_targets = std::collections::BTreeSet::new();
        for node in &state.graph.nodes {
            node_targets.insert(node.id.clone());
            if let Some(path) = &node.path {
                node_targets.insert(path.clone());
            }
            if let Some(target) = node
                .id
                .strip_prefix("symbol:")
                .or_else(|| node.id.strip_prefix("code:"))
            {
                node_targets.insert(target.to_string());
                if target.contains('#') {
                    selector_targets.insert(target.to_string());
                }
            }
        }
        let mut seen_sources = std::collections::BTreeSet::new();
        let sources = state
            .source_index
            .into_iter()
            .filter(|source| seen_sources.insert(source.path.clone()))
            .collect::<Vec<_>>();
        for source in &sources {
            selector_targets.insert(source.path.clone());
        }
        Self {
            node_targets,
            sources,
            selector_targets: selector_targets.into_iter().collect(),
        }
    }

    fn lookup(&self, target: &str) -> bool {
        self.node_targets.contains(target)
    }

    fn selectors(&self, query: &str, limit: usize) -> Vec<String> {
        let query = query.trim().to_lowercase();
        let mut values = self
            .selector_targets
            .iter()
            .filter_map(|target| selector_score(target, &query).map(|score| (score, target)))
            .collect::<Vec<_>>();
        values.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(right.1)));
        values
            .into_iter()
            .take(limit)
            .map(|(_, target)| target.clone())
            .collect()
    }
}

fn selector_score(value: &str, query: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let value = value.to_lowercase();
    let basename = value.rsplit('/').next().unwrap_or(&value);
    if value == query {
        Some(100_000)
    } else if basename == query {
        Some(90_000)
    } else if value.ends_with(query) {
        Some(80_000 - value.len() as i64)
    } else {
        value
            .find(query)
            .map(|index| 60_000 - index as i64 - value.len() as i64)
    }
}

fn diff_states(current: &MachineState, changed: &MachineState) -> (usize, usize) {
    let current = current
        .graph
        .nodes
        .iter()
        .map(|node| (&node.id, &node.hash))
        .collect::<BTreeMap<_, _>>();
    let mut added = 0;
    let mut modified = 0;
    for node in &changed.graph.nodes {
        match current.get(&node.id) {
            None => added += 1,
            Some(hash) if *hash != &node.hash => modified += 1,
            _ => {}
        }
    }
    (added, modified)
}

#[cfg(unix)]
fn peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes the supplied rusage value when it returns zero.
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if status != 0 {
        return None;
    }
    // SAFETY: the successful call above initialized usage.
    let rss = unsafe { usage.assume_init() }.ru_maxrss as u64;
    #[cfg(target_os = "macos")]
    return Some(rss);
    #[cfg(not(target_os = "macos"))]
    Some(rss.saturating_mul(1024))
}

#[cfg(windows)]
fn peak_rss_bytes() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: GetCurrentProcess returns a valid pseudo-handle for this process,
    // and counters points to an initialized structure of the supplied size.
    let status = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    (status != 0).then_some(counters.PeakWorkingSetSize as u64)
}

#[cfg(not(any(unix, windows)))]
fn peak_rss_bytes() -> Option<u64> {
    None
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub struct CandidateStore {
    state: MachineState,
    prepared: PreparedState,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
impl CandidateStore {
    #[wasm_bindgen::prelude::wasm_bindgen(constructor)]
    pub fn new(candidate: &str, bytes: &[u8]) -> Result<Self, wasm_bindgen::JsValue> {
        let candidate = candidate_from_id(candidate)
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
        let state = decode_machine_candidate(candidate, bytes)
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
        let prepared = PreparedState::new(state.clone());
        Ok(Self { state, prepared })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = validatedState)]
    pub fn validated_state(&self) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        let state = self
            .state
            .to_value()
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
        serde_wasm_bindgen::to_value(&state).map_err(|error| {
            wasm_bindgen::JsValue::from_str(&format!("failed to return State: {error}"))
        })
    }

    pub fn summary(&self) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        serde_wasm_bindgen::to_value(&serde_json::json!({
            "schema": self.state.schema,
            "node_count": self.state.graph.nodes.len(),
            "edge_count": self.state.graph.edges.len(),
            "source_count": self.prepared.sources.len(),
            "pattern_count": self.state.registered_patterns.len(),
        }))
        .map_err(|error| {
            wasm_bindgen::JsValue::from_str(&format!("failed to return summary: {error}"))
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = sourceEntries)]
    pub fn source_entries(&self) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        serde_wasm_bindgen::to_value(&self.prepared.sources).map_err(|error| {
            wasm_bindgen::JsValue::from_str(&format!("failed to return sources: {error}"))
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = graphNodes)]
    pub fn graph_nodes(&self) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        serde_wasm_bindgen::to_value(&self.state.graph.nodes).map_err(|error| {
            wasm_bindgen::JsValue::from_str(&format!("failed to return nodes: {error}"))
        })
    }

    pub fn lookup(&self, target: &str) -> bool {
        self.prepared.lookup(target)
    }

    pub fn selectors(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        serde_wasm_bindgen::to_value(&self.prepared.selectors(query, limit)).map_err(|error| {
            wasm_bindgen::JsValue::from_str(&format!("failed to return selectors: {error}"))
        })
    }
}

#[cfg(target_arch = "wasm32")]
fn candidate_from_id(value: &str) -> Result<Candidate, String> {
    match value {
        #[cfg(any(feature = "wasm-all", feature = "wasm-criv-column"))]
        "criv-column" => Ok(Candidate::CrivColumn),
        #[cfg(any(feature = "wasm-all", feature = "wasm-flatbuffers"))]
        "flatbuffers" => Ok(Candidate::Flatbuffers),
        #[cfg(any(feature = "wasm-all", feature = "wasm-json"))]
        "json" => Ok(Candidate::Json),
        #[cfg(any(feature = "wasm-all", feature = "wasm-postcard"))]
        "postcard" => Ok(Candidate::Postcard),
        #[cfg(any(feature = "wasm-all", feature = "wasm-rkyv"))]
        "rkyv" => Ok(Candidate::Rkyv),
        _ => Err(format!("unsupported State store candidate: {value}")),
    }
}

fn envelope(magic: [u8; 4], version: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(48 + payload.len());
    bytes.extend_from_slice(&magic);
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(blake3::hash(payload).as_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn open_envelope(bytes: &[u8], magic: [u8; 4], version: u32) -> Result<&[u8], String> {
    let header = bytes
        .get(..48)
        .ok_or_else(|| "truncated State header".to_string())?;
    if header[..4] != magic {
        return Err("invalid State magic".into());
    }
    let actual_version = u32::from_le_bytes(header[4..8].try_into().unwrap());
    if actual_version != version {
        return Err(format!(
            "unsupported State format version: {actual_version}"
        ));
    }
    let payload_len = u64::from_le_bytes(header[8..16].try_into().unwrap()) as usize;
    let payload = bytes
        .get(48..)
        .filter(|payload| payload.len() == payload_len)
        .ok_or_else(|| "truncated or trailing State payload".to_string())?;
    if blake3::hash(payload).as_bytes() != &header[16..48] {
        return Err("State checksum mismatch".into());
    }
    Ok(payload)
}

fn corrupt_bytes(candidate: Candidate, bytes: &[u8]) -> Vec<u8> {
    let mut corrupt = bytes.to_vec();
    let index = match candidate {
        Candidate::Json => 0,
        Candidate::CrivColumn | Candidate::Flatbuffers | Candidate::Postcard | Candidate::Rkyv => {
            corrupt.len() - 1
        }
    };
    corrupt[index] ^= 0xff;
    corrupt
}

fn unknown_version_bytes(
    candidate: Candidate,
    state: &serde_json::Value,
) -> Result<Vec<u8>, String> {
    match candidate {
        Candidate::Json => {
            let mut unknown = state.clone();
            unknown["schema"] = serde_json::Value::String("criv.state.v999".into());
            encode_json(&unknown)
        }
        Candidate::CrivColumn | Candidate::Flatbuffers | Candidate::Postcard | Candidate::Rkyv => {
            let mut unknown = encode_candidate(candidate, state)?;
            unknown[4..8].copy_from_slice(&999u32.to_le_bytes());
            Ok(unknown)
        }
    }
}

fn storage_report(
    candidate: Candidate,
    current: &[u8],
    changed: &[u8],
    snapshots: &[Vec<u8>],
) -> Result<Storage, String> {
    if let Some(magic) = candidate.partition_magic() {
        let current_partitions = open_partition_package(current, magic)?;
        let changed_partitions = open_partition_package(changed, magic)?;
        let current_hashes = current_partitions
            .iter()
            .map(|partition| partition.hash)
            .collect::<std::collections::BTreeSet<_>>();
        let new_payload_bytes = changed_partitions
            .iter()
            .filter(|partition| !current_hashes.contains(&partition.hash))
            .map(|partition| partition.payload.len())
            .sum::<usize>();
        let changed_partition_count = changed_partitions
            .iter()
            .filter(|partition| !current_hashes.contains(&partition.hash))
            .count();
        let manifest_bytes = encode_partition_manifest(magic, &changed_partitions).len();
        let mut retained_payloads = BTreeMap::<[u8; 32], usize>::new();
        let mut retained_manifests = 0usize;
        for snapshot in snapshots {
            let partitions = open_partition_package(snapshot, magic)?;
            retained_manifests += encode_partition_manifest(magic, &partitions).len();
            for partition in partitions {
                retained_payloads
                    .entry(partition.hash)
                    .or_insert(partition.payload.len());
            }
        }
        return Ok(Storage {
            stored_bytes: current_partitions
                .iter()
                .map(|partition| partition.payload.len())
                .sum::<usize>()
                + encode_partition_manifest(magic, &current_partitions).len(),
            retained_snapshot_bytes: retained_payloads.values().sum::<usize>() + retained_manifests,
            changed_publication_bytes: new_payload_bytes + manifest_bytes * 2,
            partition_count: changed_partitions.len(),
            changed_partition_count,
            reused_partition_count: changed_partitions.len() - changed_partition_count,
            edge_endpoint_width_bits: 32,
            publication_model: "content-addressed-partitions",
        });
    }
    Ok(Storage {
        stored_bytes: current.len(),
        retained_snapshot_bytes: snapshots.iter().map(Vec::len).sum(),
        changed_publication_bytes: changed.len() * 2,
        partition_count: 1,
        changed_partition_count: 1,
        reused_partition_count: 0,
        edge_endpoint_width_bits: if matches!(candidate, Candidate::Json | Candidate::Postcard) {
            0
        } else {
            32
        },
        publication_model: "whole-file",
    })
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct RkyvPartition {
    version: u32,
    block: u32,
    data: RkyvPartitionData,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
enum RkyvPartitionData {
    Strings(Vec<String>),
    Meta(RkyvMeta),
    Nodes(Vec<RkyvNodeRow>),
    Edges(Vec<RkyvEdgeRow>),
    Sources(Vec<RkyvSourceRow>),
    Patterns(Vec<RkyvPatternGroup>),
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct RkyvMeta {
    schema: u32,
    root: u32,
    architecture: Option<Vec<u8>>,
    registered_patterns: Vec<u32>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct RkyvNodeRow {
    id: u32,
    hash: u32,
    kind: u32,
    label: u32,
    path: u32,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct RkyvEdgeRow {
    from: u32,
    to: u32,
    kind: u32,
    hash: u32,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct RkyvSourceRow {
    path: u32,
    mime: u32,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct RkyvCapture {
    key: u32,
    value: u32,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct RkyvPatternMatch {
    file: u32,
    range: u32,
    captures: Vec<RkyvCapture>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct RkyvPatternGroup {
    id: u32,
    matches: Vec<RkyvPatternMatch>,
}

fn encode_rkyv_store(state: &MachineState) -> Result<Vec<u8>, String> {
    let custom = encode_column_store(state)?;
    let custom_partitions = open_column_partitions(&custom)?;
    let mut partitions = Vec::with_capacity(custom_partitions.len());
    for partition in custom_partitions {
        let value = rkyv_partition_from_column(&partition)?;
        let payload = rkyv::to_bytes::<rkyv::rancor::Error>(&value)
            .map_err(|error| format!("failed to encode rkyv partition: {error}"))?;
        partitions.push(new_partition(
            partition.kind,
            partition.block,
            payload.to_vec(),
        ));
    }
    encode_partition_package(*b"CRRK", &partitions)
}

fn decode_rkyv_store(bytes: &[u8]) -> Result<MachineState, String> {
    let archived_partitions = open_partition_package(bytes, *b"CRRK")?;
    let mut custom_partitions = Vec::with_capacity(archived_partitions.len());
    for partition in archived_partitions {
        let archived =
            rkyv::access::<ArchivedRkyvPartition, rkyv::rancor::Error>(&partition.payload)
                .map_err(|error| format!("invalid rkyv partition: {error}"))?;
        let value = rkyv::deserialize::<RkyvPartition, rkyv::rancor::Error>(archived)
            .map_err(|error| format!("failed to decode rkyv partition: {error}"))?;
        if value.version != 1 {
            return Err(format!(
                "unsupported rkyv partition version: {}",
                value.version
            ));
        }
        if value.block != partition.block {
            return Err("rkyv partition block does not match manifest".into());
        }
        let (kind, payload) = column_partition_from_rkyv(value.data)?;
        if kind != partition.kind {
            return Err("rkyv partition kind does not match manifest".into());
        }
        custom_partitions.push(new_partition(kind, partition.block, payload));
    }
    let custom = encode_partition_package(*b"CRCL", &custom_partitions)?;
    decode_column_store(&custom)
}

fn rkyv_partition_from_column(partition: &StoredPartition) -> Result<RkyvPartition, String> {
    let mut reader = BinaryReader::new(&partition.payload);
    let data = match partition.kind {
        PARTITION_STRINGS => RkyvPartitionData::Strings(reader.strings()?),
        PARTITION_META => {
            let schema = reader.u32()?;
            let root = reader.u32()?;
            let architecture = reader.optional_bytes()?.map(ToOwned::to_owned);
            let count = reader.u32()? as usize;
            RkyvPartitionData::Meta(RkyvMeta {
                schema,
                root,
                architecture,
                registered_patterns: read_u32s(&mut reader, count)?,
            })
        }
        PARTITION_NODES => {
            let count = reader.u32()? as usize;
            let ids = read_u32s(&mut reader, count)?;
            let hashes = read_u32s(&mut reader, count)?;
            let kinds = read_u32s(&mut reader, count)?;
            let labels = read_u32s(&mut reader, count)?;
            let paths = read_u32s(&mut reader, count)?;
            RkyvPartitionData::Nodes(
                (0..count)
                    .map(|index| RkyvNodeRow {
                        id: ids[index],
                        hash: hashes[index],
                        kind: kinds[index],
                        label: labels[index],
                        path: paths[index],
                    })
                    .collect(),
            )
        }
        PARTITION_EDGES => {
            let count = reader.u32()? as usize;
            let from = read_u32s(&mut reader, count)?;
            let to = read_u32s(&mut reader, count)?;
            let kinds = read_u32s(&mut reader, count)?;
            let hashes = read_u32s(&mut reader, count)?;
            RkyvPartitionData::Edges(
                (0..count)
                    .map(|index| RkyvEdgeRow {
                        from: from[index],
                        to: to[index],
                        kind: kinds[index],
                        hash: hashes[index],
                    })
                    .collect(),
            )
        }
        PARTITION_SOURCES => {
            let count = reader.u32()? as usize;
            let paths = read_u32s(&mut reader, count)?;
            let mimes = read_u32s(&mut reader, count)?;
            RkyvPartitionData::Sources(
                (0..count)
                    .map(|index| RkyvSourceRow {
                        path: paths[index],
                        mime: mimes[index],
                    })
                    .collect(),
            )
        }
        PARTITION_PATTERNS => {
            let group_count = reader.u32()? as usize;
            let mut groups = Vec::with_capacity(group_count);
            for _ in 0..group_count {
                let id = reader.u32()?;
                let match_count = reader.u32()? as usize;
                let mut matches = Vec::with_capacity(match_count);
                for _ in 0..match_count {
                    let file = reader.u32()?;
                    let range = reader.u32()?;
                    let capture_count = reader.u32()? as usize;
                    let mut captures = Vec::with_capacity(capture_count);
                    for _ in 0..capture_count {
                        captures.push(RkyvCapture {
                            key: reader.u32()?,
                            value: reader.u32()?,
                        });
                    }
                    matches.push(RkyvPatternMatch {
                        file,
                        range,
                        captures,
                    });
                }
                groups.push(RkyvPatternGroup { id, matches });
            }
            RkyvPartitionData::Patterns(groups)
        }
        kind => return Err(format!("unsupported rkyv column partition kind {kind}")),
    };
    reader.finish()?;
    Ok(RkyvPartition {
        version: 1,
        block: partition.block,
        data,
    })
}

fn column_partition_from_rkyv(data: RkyvPartitionData) -> Result<(u8, Vec<u8>), String> {
    let mut writer = BinaryWriter::default();
    let kind = match data {
        RkyvPartitionData::Strings(values) => {
            writer.strings(&values);
            PARTITION_STRINGS
        }
        RkyvPartitionData::Meta(meta) => {
            writer.u32(meta.schema);
            writer.u32(meta.root);
            writer.optional_bytes(meta.architecture.as_deref());
            writer.u32(meta.registered_patterns.len() as u32);
            for value in meta.registered_patterns {
                writer.u32(value);
            }
            PARTITION_META
        }
        RkyvPartitionData::Nodes(rows) => {
            writer.u32(rows.len() as u32);
            for row in &rows {
                writer.u32(row.id);
            }
            for row in &rows {
                writer.u32(row.hash);
            }
            for row in &rows {
                writer.u32(row.kind);
            }
            for row in &rows {
                writer.u32(row.label);
            }
            for row in &rows {
                writer.u32(row.path);
            }
            PARTITION_NODES
        }
        RkyvPartitionData::Edges(rows) => {
            writer.u32(rows.len() as u32);
            for row in &rows {
                writer.u32(row.from);
            }
            for row in &rows {
                writer.u32(row.to);
            }
            for row in &rows {
                writer.u32(row.kind);
            }
            for row in &rows {
                writer.u32(row.hash);
            }
            PARTITION_EDGES
        }
        RkyvPartitionData::Sources(rows) => {
            writer.u32(rows.len() as u32);
            for row in &rows {
                writer.u32(row.path);
            }
            for row in &rows {
                writer.u32(row.mime);
            }
            PARTITION_SOURCES
        }
        RkyvPartitionData::Patterns(groups) => {
            writer.u32(groups.len() as u32);
            for group in groups {
                writer.u32(group.id);
                writer.u32(group.matches.len() as u32);
                for pattern_match in group.matches {
                    writer.u32(pattern_match.file);
                    writer.u32(pattern_match.range);
                    writer.u32(pattern_match.captures.len() as u32);
                    for capture in pattern_match.captures {
                        writer.u32(capture.key);
                        writer.u32(capture.value);
                    }
                }
            }
            PARTITION_PATTERNS
        }
    };
    Ok((kind, writer.finish()))
}

const PARTITION_STRINGS: u8 = 1;
const PARTITION_META: u8 = 2;
const PARTITION_NODES: u8 = 3;
const PARTITION_EDGES: u8 = 4;
const PARTITION_SOURCES: u8 = 5;
const PARTITION_PATTERNS: u8 = 6;
const BLOCK_ROWS: usize = 256;
const NONE_INDEX: u32 = u32::MAX;

#[derive(Debug)]
struct StoredPartition {
    kind: u8,
    block: u32,
    hash: [u8; 32],
    payload: Vec<u8>,
}

#[derive(Default)]
struct Interner {
    values: Vec<String>,
    indexes: BTreeMap<String, u32>,
}

impl Interner {
    fn intern(&mut self, value: &str) -> u32 {
        if let Some(index) = self.indexes.get(value) {
            return *index;
        }
        let index = self.values.len() as u32;
        self.values.push(value.to_string());
        self.indexes.insert(value.to_string(), index);
        index
    }

    fn optional(&mut self, value: Option<&str>) -> u32 {
        value.map_or(NONE_INDEX, |value| self.intern(value))
    }
}

fn encode_column_store(state: &MachineState) -> Result<Vec<u8>, String> {
    let mut strings = Interner::default();
    for node in &state.graph.nodes {
        for value in [&node.id, &node.hash, &node.kind, &node.label] {
            strings.intern(value);
        }
        strings.optional(node.path.as_deref());
    }
    for edge in &state.graph.edges {
        strings.intern(&edge.kind);
        strings.intern(&edge.hash);
    }
    for source in &state.source_index {
        strings.intern(&source.path);
        strings.optional(source.mime.as_deref());
    }
    for pattern in &state.registered_patterns {
        strings.intern(pattern);
    }
    for (id, matches) in &state.patterns {
        strings.intern(id);
        for pattern_match in matches {
            strings.intern(&pattern_match.file);
            strings.optional(pattern_match.range.as_deref());
            for (key, value) in &pattern_match.captures {
                strings.intern(key);
                strings.intern(value);
            }
        }
    }
    let root = strings.intern(&state.graph.root);
    let schema = strings.intern(&state.schema);

    let mut partitions = Vec::new();
    let mut writer = BinaryWriter::default();
    writer.strings(&strings.values);
    partitions.push(new_partition(PARTITION_STRINGS, 0, writer.finish()));

    let mut writer = BinaryWriter::default();
    writer.u32(schema);
    writer.u32(root);
    writer.optional_bytes(state.architecture_json.as_deref());
    writer.u32(state.registered_patterns.len() as u32);
    for value in &state.registered_patterns {
        writer.u32(strings.indexes[value]);
    }
    partitions.push(new_partition(PARTITION_META, 0, writer.finish()));

    for (block, nodes) in state.graph.nodes.chunks(BLOCK_ROWS).enumerate() {
        let mut writer = BinaryWriter::default();
        writer.u32(nodes.len() as u32);
        for node in nodes {
            writer.u32(strings.indexes[&node.id]);
        }
        for node in nodes {
            writer.u32(strings.indexes[&node.hash]);
        }
        for node in nodes {
            writer.u32(strings.indexes[&node.kind]);
        }
        for node in nodes {
            writer.u32(strings.indexes[&node.label]);
        }
        for node in nodes {
            writer.u32(
                node.path
                    .as_ref()
                    .map_or(NONE_INDEX, |value| strings.indexes[value]),
            );
        }
        partitions.push(new_partition(
            PARTITION_NODES,
            block as u32,
            writer.finish(),
        ));
    }

    let node_indexes = state
        .graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index as u32))
        .collect::<BTreeMap<_, _>>();
    for (block, edges) in state.graph.edges.chunks(BLOCK_ROWS).enumerate() {
        let mut writer = BinaryWriter::default();
        writer.u32(edges.len() as u32);
        for edge in edges {
            writer.u32(
                *node_indexes
                    .get(edge.from.as_str())
                    .ok_or_else(|| format!("edge source {} has no node row", edge.from))?,
            );
        }
        for edge in edges {
            writer.u32(
                *node_indexes
                    .get(edge.to.as_str())
                    .ok_or_else(|| format!("edge target {} has no node row", edge.to))?,
            );
        }
        for edge in edges {
            writer.u32(strings.indexes[&edge.kind]);
        }
        for edge in edges {
            writer.u32(strings.indexes[&edge.hash]);
        }
        partitions.push(new_partition(
            PARTITION_EDGES,
            block as u32,
            writer.finish(),
        ));
    }

    for (block, sources) in state.source_index.chunks(BLOCK_ROWS).enumerate() {
        let mut writer = BinaryWriter::default();
        writer.u32(sources.len() as u32);
        for source in sources {
            writer.u32(strings.indexes[&source.path]);
        }
        for source in sources {
            writer.u32(
                source
                    .mime
                    .as_ref()
                    .map_or(NONE_INDEX, |value| strings.indexes[value]),
            );
        }
        partitions.push(new_partition(
            PARTITION_SOURCES,
            block as u32,
            writer.finish(),
        ));
    }

    let mut writer = BinaryWriter::default();
    writer.u32(state.patterns.len() as u32);
    for (id, matches) in &state.patterns {
        writer.u32(strings.indexes[id]);
        writer.u32(matches.len() as u32);
        for pattern_match in matches {
            writer.u32(strings.indexes[&pattern_match.file]);
            writer.u32(
                pattern_match
                    .range
                    .as_ref()
                    .map_or(NONE_INDEX, |value| strings.indexes[value]),
            );
            writer.u32(pattern_match.captures.len() as u32);
            for (key, value) in &pattern_match.captures {
                writer.u32(strings.indexes[key]);
                writer.u32(strings.indexes[value]);
            }
        }
    }
    partitions.push(new_partition(PARTITION_PATTERNS, 0, writer.finish()));
    encode_partition_package(*b"CRCL", &partitions)
}

fn new_partition(kind: u8, block: u32, payload: Vec<u8>) -> StoredPartition {
    StoredPartition {
        kind,
        block,
        hash: *blake3::hash(&payload).as_bytes(),
        payload,
    }
}

fn encode_partition_package(
    magic: [u8; 4],
    partitions: &[StoredPartition],
) -> Result<Vec<u8>, String> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&(partitions.len() as u32).to_le_bytes());
    for partition in partitions {
        payload.push(partition.kind);
        payload.extend_from_slice(&partition.block.to_le_bytes());
        payload.extend_from_slice(&(partition.payload.len() as u64).to_le_bytes());
        payload.extend_from_slice(&partition.hash);
        payload.extend_from_slice(&partition.payload);
    }
    Ok(envelope(magic, 1, &payload))
}

fn open_column_partitions(bytes: &[u8]) -> Result<Vec<StoredPartition>, String> {
    open_partition_package(bytes, *b"CRCL")
}

fn open_partition_package(bytes: &[u8], magic: [u8; 4]) -> Result<Vec<StoredPartition>, String> {
    let payload = open_envelope(bytes, magic, 1)?;
    let mut reader = BinaryReader::new(payload);
    let count = reader.u32()? as usize;
    let mut partitions = Vec::with_capacity(count);
    for _ in 0..count {
        let kind = reader.u8()?;
        let block = reader.u32()?;
        let len = reader.u64()? as usize;
        let expected_hash: [u8; 32] = reader.bytes(32)?.try_into().unwrap();
        let payload = reader.bytes(len)?.to_vec();
        if blake3::hash(&payload).as_bytes() != &expected_hash {
            return Err("column partition checksum mismatch".into());
        }
        partitions.push(StoredPartition {
            kind,
            block,
            hash: expected_hash,
            payload,
        });
    }
    reader.finish()?;
    partitions.sort_by_key(|partition| (partition.kind, partition.block));
    Ok(partitions)
}

fn encode_flatbuffers_store(state: &MachineState) -> Result<Vec<u8>, String> {
    let custom = encode_column_store(state)?;
    let custom_partitions = open_column_partitions(&custom)?;
    let mut partitions = Vec::with_capacity(custom_partitions.len());
    for partition in custom_partitions {
        let payload = flatbuffer_from_column_partition(&partition)?;
        partitions.push(new_partition(partition.kind, partition.block, payload));
    }
    encode_partition_package(*b"CRFB", &partitions)
}

fn decode_flatbuffers_store(bytes: &[u8]) -> Result<MachineState, String> {
    let flat_partitions = open_partition_package(bytes, *b"CRFB")?;
    let mut custom_partitions = Vec::with_capacity(flat_partitions.len());
    for partition in flat_partitions {
        let payload = column_partition_from_flatbuffer(&partition)?;
        custom_partitions.push(new_partition(partition.kind, partition.block, payload));
    }
    let custom = encode_partition_package(*b"CRCL", &custom_partitions)?;
    decode_column_store(&custom)
}

fn flatbuffer_from_column_partition(partition: &StoredPartition) -> Result<Vec<u8>, String> {
    let mut builder = flatbuffers::FlatBufferBuilder::new();
    let mut reader = BinaryReader::new(&partition.payload);
    let (data_type, data) = match partition.kind {
        PARTITION_STRINGS => {
            let values = reader.strings()?;
            reader.finish()?;
            let offsets = values
                .iter()
                .map(|value| builder.create_string(value))
                .collect::<Vec<_>>();
            let values = builder.create_vector(&offsets);
            let data = fb::StringTable::create(
                &mut builder,
                &fb::StringTableArgs {
                    values: Some(values),
                },
            );
            (fb::PartitionData::StringTable, data.as_union_value())
        }
        PARTITION_META => {
            let schema = reader.u32()?;
            let root = reader.u32()?;
            let architecture_bytes = reader.optional_bytes()?;
            let architecture = architecture_bytes.map(|value| builder.create_vector(value));
            let count = reader.u32()? as usize;
            let registered = read_u32s(&mut reader, count)?;
            reader.finish()?;
            let registered_patterns = builder.create_vector(&registered);
            let data = fb::Metadata::create(
                &mut builder,
                &fb::MetadataArgs {
                    schema,
                    root,
                    architecture,
                    has_architecture: architecture_bytes.is_some(),
                    registered_patterns: Some(registered_patterns),
                },
            );
            (fb::PartitionData::Metadata, data.as_union_value())
        }
        PARTITION_NODES => {
            let count = reader.u32()? as usize;
            let ids = read_u32s(&mut reader, count)?;
            let hashes = read_u32s(&mut reader, count)?;
            let kinds = read_u32s(&mut reader, count)?;
            let labels = read_u32s(&mut reader, count)?;
            let paths = read_u32s(&mut reader, count)?;
            reader.finish()?;
            let rows = (0..count)
                .map(|index| {
                    fb::NodeRow::new(
                        ids[index],
                        hashes[index],
                        kinds[index],
                        labels[index],
                        paths[index],
                    )
                })
                .collect::<Vec<_>>();
            let rows = builder.create_vector(&rows);
            let data = fb::NodeBlock::create(&mut builder, &fb::NodeBlockArgs { rows: Some(rows) });
            (fb::PartitionData::NodeBlock, data.as_union_value())
        }
        PARTITION_EDGES => {
            let count = reader.u32()? as usize;
            let from = read_u32s(&mut reader, count)?;
            let to = read_u32s(&mut reader, count)?;
            let kinds = read_u32s(&mut reader, count)?;
            let hashes = read_u32s(&mut reader, count)?;
            reader.finish()?;
            let rows = (0..count)
                .map(|index| fb::EdgeRow::new(from[index], to[index], kinds[index], hashes[index]))
                .collect::<Vec<_>>();
            let rows = builder.create_vector(&rows);
            let data = fb::EdgeBlock::create(&mut builder, &fb::EdgeBlockArgs { rows: Some(rows) });
            (fb::PartitionData::EdgeBlock, data.as_union_value())
        }
        PARTITION_SOURCES => {
            let count = reader.u32()? as usize;
            let paths = read_u32s(&mut reader, count)?;
            let mimes = read_u32s(&mut reader, count)?;
            reader.finish()?;
            let rows = (0..count)
                .map(|index| fb::SourceRow::new(paths[index], mimes[index]))
                .collect::<Vec<_>>();
            let rows = builder.create_vector(&rows);
            let data =
                fb::SourceBlock::create(&mut builder, &fb::SourceBlockArgs { rows: Some(rows) });
            (fb::PartitionData::SourceBlock, data.as_union_value())
        }
        PARTITION_PATTERNS => {
            let group_count = reader.u32()? as usize;
            let mut groups = Vec::with_capacity(group_count);
            for _ in 0..group_count {
                let id = reader.u32()?;
                let match_count = reader.u32()? as usize;
                let mut matches = Vec::with_capacity(match_count);
                for _ in 0..match_count {
                    let file = reader.u32()?;
                    let range = reader.u32()?;
                    let capture_count = reader.u32()? as usize;
                    let captures = (0..capture_count)
                        .map(|_| {
                            Ok(fb::Capture::create(
                                &mut builder,
                                &fb::CaptureArgs {
                                    key: reader.u32()?,
                                    value: reader.u32()?,
                                },
                            ))
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    let captures = builder.create_vector(&captures);
                    matches.push(fb::PatternMatch::create(
                        &mut builder,
                        &fb::PatternMatchArgs {
                            file,
                            range,
                            captures: Some(captures),
                        },
                    ));
                }
                let matches = builder.create_vector(&matches);
                groups.push(fb::PatternGroup::create(
                    &mut builder,
                    &fb::PatternGroupArgs {
                        id,
                        matches: Some(matches),
                    },
                ));
            }
            reader.finish()?;
            let groups = builder.create_vector(&groups);
            let data = fb::Patterns::create(
                &mut builder,
                &fb::PatternsArgs {
                    groups: Some(groups),
                },
            );
            (fb::PartitionData::Patterns, data.as_union_value())
        }
        kind => return Err(format!("unsupported column partition kind {kind}")),
    };
    let root = fb::Partition::create(
        &mut builder,
        &fb::PartitionArgs {
            version: 1,
            block: partition.block,
            data_type,
            data: Some(data),
        },
    );
    fb::finish_partition_buffer(&mut builder, root);
    Ok(builder.finished_data().to_vec())
}

fn column_partition_from_flatbuffer(partition: &StoredPartition) -> Result<Vec<u8>, String> {
    let root = fb::root_as_partition(&partition.payload)
        .map_err(|error| format!("invalid FlatBuffers partition: {error}"))?;
    if root.version() != 1 {
        return Err(format!(
            "unsupported FlatBuffers partition version: {}",
            root.version()
        ));
    }
    if root.block() != partition.block {
        return Err("FlatBuffers partition block does not match manifest".into());
    }
    let mut writer = BinaryWriter::default();
    match partition.kind {
        PARTITION_STRINGS => {
            let data = root
                .data_as_string_table()
                .ok_or_else(|| "FlatBuffers string partition has wrong union type".to_string())?;
            let values = data
                .values()
                .map(|values| values.iter().map(str::to_string).collect::<Vec<_>>())
                .unwrap_or_default();
            writer.strings(&values);
        }
        PARTITION_META => {
            let data = root
                .data_as_metadata()
                .ok_or_else(|| "FlatBuffers metadata partition has wrong union type".to_string())?;
            writer.u32(data.schema());
            writer.u32(data.root());
            let architecture = data.architecture().map(|value| value.bytes());
            if data.has_architecture() != architecture.is_some() {
                return Err("FlatBuffers architecture presence flag is inconsistent".into());
            }
            writer.optional_bytes(architecture);
            let registered = data.registered_patterns();
            writer.u32(registered.map_or(0, |values| values.len()) as u32);
            if let Some(registered) = registered {
                for value in registered {
                    writer.u32(value);
                }
            }
        }
        PARTITION_NODES => {
            let rows = root
                .data_as_node_block()
                .and_then(|data| data.rows())
                .ok_or_else(|| "FlatBuffers node partition has no rows".to_string())?;
            writer.u32(rows.len() as u32);
            for row in rows {
                writer.u32(row.id());
            }
            for row in rows {
                writer.u32(row.hash());
            }
            for row in rows {
                writer.u32(row.kind());
            }
            for row in rows {
                writer.u32(row.label());
            }
            for row in rows {
                writer.u32(row.path());
            }
        }
        PARTITION_EDGES => {
            let rows = root
                .data_as_edge_block()
                .and_then(|data| data.rows())
                .ok_or_else(|| "FlatBuffers edge partition has no rows".to_string())?;
            writer.u32(rows.len() as u32);
            for row in rows {
                writer.u32(row.from());
            }
            for row in rows {
                writer.u32(row.to());
            }
            for row in rows {
                writer.u32(row.kind());
            }
            for row in rows {
                writer.u32(row.hash());
            }
        }
        PARTITION_SOURCES => {
            let rows = root
                .data_as_source_block()
                .and_then(|data| data.rows())
                .ok_or_else(|| "FlatBuffers source partition has no rows".to_string())?;
            writer.u32(rows.len() as u32);
            for row in rows {
                writer.u32(row.path());
            }
            for row in rows {
                writer.u32(row.mime());
            }
        }
        PARTITION_PATTERNS => {
            let groups = root.data_as_patterns().and_then(|data| data.groups());
            writer.u32(groups.map_or(0, |values| values.len()) as u32);
            if let Some(groups) = groups {
                for group in groups {
                    writer.u32(group.id());
                    let matches = group.matches();
                    writer.u32(matches.map_or(0, |values| values.len()) as u32);
                    if let Some(matches) = matches {
                        for pattern_match in matches {
                            writer.u32(pattern_match.file());
                            writer.u32(pattern_match.range());
                            let captures = pattern_match.captures();
                            writer.u32(captures.map_or(0, |values| values.len()) as u32);
                            if let Some(captures) = captures {
                                for capture in captures {
                                    writer.u32(capture.key());
                                    writer.u32(capture.value());
                                }
                            }
                        }
                    }
                }
            }
        }
        kind => return Err(format!("unsupported FlatBuffers partition kind {kind}")),
    }
    Ok(writer.finish())
}

fn decode_column_store(bytes: &[u8]) -> Result<MachineState, String> {
    let partitions = open_column_partitions(bytes)?;
    let strings_partition = one_partition(&partitions, PARTITION_STRINGS)?;
    let mut reader = BinaryReader::new(&strings_partition.payload);
    let strings = reader.strings()?;
    reader.finish()?;
    let string = |index: u32| -> Result<String, String> {
        strings
            .get(index as usize)
            .cloned()
            .ok_or_else(|| format!("string index {index} is out of range"))
    };
    let optional_string = |index: u32| -> Result<Option<String>, String> {
        if index == NONE_INDEX {
            Ok(None)
        } else {
            string(index).map(Some)
        }
    };

    let meta = one_partition(&partitions, PARTITION_META)?;
    let mut reader = BinaryReader::new(&meta.payload);
    let schema = string(reader.u32()?)?;
    let root = string(reader.u32()?)?;
    let architecture_json = reader.optional_bytes()?.map(ToOwned::to_owned);
    let registered_count = reader.u32()? as usize;
    let mut registered_patterns = Vec::with_capacity(registered_count);
    for _ in 0..registered_count {
        registered_patterns.push(string(reader.u32()?)?);
    }
    reader.finish()?;

    let mut nodes = Vec::new();
    for partition in partitions
        .iter()
        .filter(|value| value.kind == PARTITION_NODES)
    {
        let mut reader = BinaryReader::new(&partition.payload);
        let count = reader.u32()? as usize;
        let ids = read_u32s(&mut reader, count)?;
        let hashes = read_u32s(&mut reader, count)?;
        let kinds = read_u32s(&mut reader, count)?;
        let labels = read_u32s(&mut reader, count)?;
        let paths = read_u32s(&mut reader, count)?;
        reader.finish()?;
        for index in 0..count {
            nodes.push(Node {
                id: string(ids[index])?,
                hash: string(hashes[index])?,
                kind: string(kinds[index])?,
                label: string(labels[index])?,
                path: optional_string(paths[index])?,
            });
        }
    }

    let mut edges = Vec::new();
    for partition in partitions
        .iter()
        .filter(|value| value.kind == PARTITION_EDGES)
    {
        let mut reader = BinaryReader::new(&partition.payload);
        let count = reader.u32()? as usize;
        let from = read_u32s(&mut reader, count)?;
        let to = read_u32s(&mut reader, count)?;
        let kinds = read_u32s(&mut reader, count)?;
        let hashes = read_u32s(&mut reader, count)?;
        reader.finish()?;
        for index in 0..count {
            let source = nodes
                .get(from[index] as usize)
                .ok_or_else(|| "edge source row is out of range".to_string())?;
            let target = nodes
                .get(to[index] as usize)
                .ok_or_else(|| "edge target row is out of range".to_string())?;
            edges.push(Edge {
                from: source.id.clone(),
                to: target.id.clone(),
                kind: string(kinds[index])?,
                hash: string(hashes[index])?,
            });
        }
    }

    let mut source_index = Vec::new();
    for partition in partitions
        .iter()
        .filter(|value| value.kind == PARTITION_SOURCES)
    {
        let mut reader = BinaryReader::new(&partition.payload);
        let count = reader.u32()? as usize;
        let paths = read_u32s(&mut reader, count)?;
        let mimes = read_u32s(&mut reader, count)?;
        reader.finish()?;
        for index in 0..count {
            source_index.push(SourceIndexEntry {
                path: string(paths[index])?,
                mime: optional_string(mimes[index])?,
            });
        }
    }

    let patterns_partition = one_partition(&partitions, PARTITION_PATTERNS)?;
    let mut reader = BinaryReader::new(&patterns_partition.payload);
    let pattern_count = reader.u32()? as usize;
    let mut patterns = BTreeMap::new();
    for _ in 0..pattern_count {
        let id = string(reader.u32()?)?;
        let match_count = reader.u32()? as usize;
        let mut matches = Vec::with_capacity(match_count);
        for _ in 0..match_count {
            let file = string(reader.u32()?)?;
            let range = optional_string(reader.u32()?)?;
            let capture_count = reader.u32()? as usize;
            let mut captures = BTreeMap::new();
            for _ in 0..capture_count {
                captures.insert(string(reader.u32()?)?, string(reader.u32()?)?);
            }
            matches.push(PatternMatch {
                file,
                range,
                captures,
            });
        }
        patterns.insert(id, matches);
    }
    reader.finish()?;
    if schema != STATE_SCHEMA {
        return Err(format!("unsupported State schema: {schema}"));
    }
    Ok(MachineState {
        schema,
        architecture_json,
        graph: Graph { root, nodes, edges },
        registered_patterns,
        patterns,
        source_index,
    })
}

fn one_partition(partitions: &[StoredPartition], kind: u8) -> Result<&StoredPartition, String> {
    let mut matches = partitions.iter().filter(|value| value.kind == kind);
    let partition = matches
        .next()
        .ok_or_else(|| format!("missing column partition {kind}"))?;
    if matches.next().is_some() {
        return Err(format!("duplicate singleton column partition {kind}"));
    }
    Ok(partition)
}

fn read_u32s(reader: &mut BinaryReader<'_>, count: usize) -> Result<Vec<u32>, String> {
    (0..count).map(|_| reader.u32()).collect()
}

#[derive(Default)]
struct BinaryWriter {
    bytes: Vec<u8>,
}

impl BinaryWriter {
    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn optional_bytes(&mut self, value: Option<&[u8]>) {
        match value {
            Some(value) => {
                self.u32(value.len() as u32);
                self.bytes.extend_from_slice(value);
            }
            None => self.u32(NONE_INDEX),
        }
    }

    fn strings(&mut self, values: &[String]) {
        self.u32(values.len() as u32);
        for value in values {
            self.u32(value.len() as u32);
            self.bytes.extend_from_slice(value.as_bytes());
        }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

struct BinaryReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> BinaryReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| "column offset overflow".to_string())?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| "truncated column data".to_string())?;
        self.offset = end;
        Ok(value)
    }

    fn optional_bytes(&mut self) -> Result<Option<&'a [u8]>, String> {
        let len = self.u32()?;
        if len == NONE_INDEX {
            Ok(None)
        } else {
            self.bytes(len as usize).map(Some)
        }
    }

    fn strings(&mut self) -> Result<Vec<String>, String> {
        let count = self.u32()? as usize;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let len = self.u32()? as usize;
            let value = std::str::from_utf8(self.bytes(len)?)
                .map_err(|error| format!("invalid UTF-8 in string table: {error}"))?;
            values.push(value.to_string());
        }
        Ok(values)
    }

    fn finish(&self) -> Result<(), String> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err("trailing column data".into())
        }
    }
}

fn decode_json(bytes: &[u8]) -> Result<serde_json::Value, String> {
    let state = serde_json::from_slice::<serde_json::Value>(bytes)
        .map_err(|error| format!("invalid JSON State: {error}"))?;
    let schema = state
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>");
    if schema != STATE_SCHEMA {
        return Err(format!("unsupported State schema: {schema}"));
    }
    Ok(state)
}

fn interrupted_publication_keeps_current(
    candidate: Candidate,
    current_bytes: &[u8],
    changed_bytes: &[u8],
) -> Result<bool, String> {
    let root = tempfile::tempdir()
        .map_err(|error| format!("failed to create publication test directory: {error}"))?;
    publish_encoded(candidate, root.path(), current_bytes, "latest", "initial")?;
    let extension = if candidate.partition_magic().is_some() {
        "manifest"
    } else {
        "bin"
    };
    let current = root.path().join(format!("latest.{extension}"));
    let before = fs::read(&current)
        .map_err(|error| format!("failed to read current publication: {error}"))?;
    let next = if let Some(magic) = candidate.partition_magic() {
        let partitions = open_partition_package(changed_bytes, magic)?;
        write_partition_objects(root.path(), &partitions)?;
        encode_partition_manifest(magic, &partitions)
    } else {
        changed_bytes.to_vec()
    };
    let interrupted = root.path().join(format!(".latest.{extension}.interrupted"));
    fs::write(&interrupted, &next[..next.len() / 2])
        .map_err(|error| format!("failed to write interrupted publication: {error}"))?;
    fs::read(&current)
        .map(|current| current == before)
        .map_err(|error| format!("failed to read publication test State: {error}"))
}
