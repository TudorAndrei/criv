use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Parser;
use criv_state_wire::STATE_SCHEMA;
use serde::Serialize;

const OUTPUT_SCHEMA: &str = "criv.state-storage-baseline.v1";

#[derive(Debug, Parser)]
#[command(
    name = "criv-state-storage-baseline",
    about = "Measure the current JSON State shape and native load cost"
)]
struct Args {
    /// Generated .criv/state.json file to measure.
    #[arg(long, required = true)]
    state: PathBuf,
    /// Number of recorded native load samples.
    #[arg(long, default_value_t = 5)]
    samples: usize,
    /// Permit fewer than three samples for smoke tests only.
    #[arg(long)]
    allow_low_samples: bool,
}

#[derive(Debug, Serialize)]
struct StateShape {
    state_bytes: usize,
    compact_bytes: usize,
    architecture_compact_bytes: usize,
    node_count: usize,
    edge_count: usize,
    source_count: usize,
    pattern_count: usize,
    edge_endpoint_occurrences: usize,
    edge_endpoint_bytes: usize,
    unique_edge_endpoint_count: usize,
    unique_edge_endpoint_bytes: usize,
    repeated_edge_endpoint_bytes: usize,
    value_strings: StringShape,
    object_keys: StringShape,
}

#[derive(Debug, Serialize)]
struct StringShape {
    occurrences: usize,
    bytes: usize,
    unique_count: usize,
    unique_bytes: usize,
    repeated_bytes: usize,
}

#[derive(Debug, Serialize)]
struct TimingSummary {
    samples: usize,
    raw_seconds: Vec<f64>,
    minimum_seconds: f64,
    median_seconds: f64,
    maximum_seconds: f64,
    median_absolute_deviation_seconds: f64,
}

#[derive(Debug, Serialize)]
struct NativeLoadBaseline {
    definition: &'static str,
    cache_state: &'static str,
    timing: TimingSummary,
    peak_rss_bytes: Option<u64>,
    peak_memory_note: &'static str,
}

#[derive(Debug, Serialize)]
struct BaselineDocument {
    schema: &'static str,
    state: String,
    shape: StateShape,
    native_load_validate: NativeLoadBaseline,
}

#[derive(Default)]
struct StringInventory {
    occurrences: usize,
    bytes: usize,
    values: BTreeMap<String, usize>,
}

impl StringInventory {
    fn record(&mut self, value: &str) {
        self.occurrences += 1;
        self.bytes += value.len();
        *self.values.entry(value.to_string()).or_default() += 1;
    }

    fn shape(self) -> StringShape {
        let unique_bytes = self.values.keys().map(String::len).sum();
        StringShape {
            occurrences: self.occurrences,
            bytes: self.bytes,
            unique_count: self.values.len(),
            unique_bytes,
            repeated_bytes: self.bytes.saturating_sub(unique_bytes),
        }
    }
}

fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("criv-state-storage-baseline: {error}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    if args.samples == 0 || (args.samples < 3 && !args.allow_low_samples) {
        return Err(
            "samples must be at least 3 (use --allow-low-samples only for smoke tests)".into(),
        );
    }
    let state = fs::canonicalize(&args.state)
        .map_err(|error| format!("failed to resolve State {}: {error}", args.state.display()))?;
    let bytes = fs::read(&state)
        .map_err(|error| format!("failed to read State {}: {error}", state.display()))?;
    let shape = analyze_state(&bytes)?;

    load_and_validate(&state)?;
    let mut samples = Vec::with_capacity(args.samples);
    for _ in 0..args.samples {
        let start = Instant::now();
        load_and_validate(&state)?;
        samples.push(start.elapsed().as_secs_f64());
    }
    let document = BaselineDocument {
        schema: OUTPUT_SCHEMA,
        state: state.display().to_string(),
        shape,
        native_load_validate: NativeLoadBaseline {
            definition: "read the complete file, decode serde_json::Value, and validate the State schema",
            cache_state: "warm operating-system file cache after one untimed load",
            timing: timing_summary(samples)?,
            peak_rss_bytes: None,
            peak_memory_note: "No portable external peak-RSS measure isolates this in-process operation; the benchmark records null instead of an estimate.",
        },
    };
    serde_json::to_writer_pretty(std::io::stdout().lock(), &document)
        .map_err(|error| format!("failed to write baseline JSON: {error}"))?;
    println!();
    Ok(())
}

fn load_and_validate(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read State {}: {error}", path.display()))?;
    let state = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|error| format!("invalid criv state JSON: {error}"))?;
    let schema = state
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>");
    if schema != STATE_SCHEMA {
        return Err(format!("unsupported criv state schema: {schema}"));
    }
    black_box(state);
    Ok(())
}

fn timing_summary(values: Vec<f64>) -> Result<TimingSummary, String> {
    if values.is_empty() {
        return Err("native load measurement needs at least one sample".into());
    }
    let mut ordered = values.clone();
    ordered.sort_by(f64::total_cmp);
    let median_value = median(&ordered);
    let mut deviations = ordered
        .iter()
        .map(|value| (value - median_value).abs())
        .collect::<Vec<_>>();
    deviations.sort_by(f64::total_cmp);
    Ok(TimingSummary {
        samples: values.len(),
        raw_seconds: values,
        minimum_seconds: ordered[0],
        median_seconds: median_value,
        maximum_seconds: ordered[ordered.len() - 1],
        median_absolute_deviation_seconds: median(&deviations),
    })
}

fn median(values: &[f64]) -> f64 {
    if values.len() % 2 == 1 {
        values[values.len() / 2]
    } else {
        (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
    }
}

fn analyze_state(bytes: &[u8]) -> Result<StateShape, String> {
    let state = serde_json::from_slice::<serde_json::Value>(bytes)
        .map_err(|error| format!("invalid criv state JSON: {error}"))?;
    let schema = state
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>");
    if schema != STATE_SCHEMA {
        return Err(format!("unsupported criv state schema: {schema}"));
    }

    let nodes = state
        .pointer("/graph/nodes")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let edges = state
        .pointer("/graph/edges")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let source_count = state
        .get("source-index")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let pattern_count = state
        .get("registered-patterns")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let mut endpoint_occurrences = 0usize;
    let mut endpoint_bytes = 0usize;
    let mut unique_endpoints = BTreeSet::new();
    for edge in edges {
        for key in ["from", "to"] {
            if let Some(endpoint) = edge.get(key).and_then(serde_json::Value::as_str) {
                endpoint_occurrences += 1;
                endpoint_bytes += endpoint.len();
                unique_endpoints.insert(endpoint);
            }
        }
    }
    let unique_endpoint_bytes = unique_endpoints.iter().map(|value| value.len()).sum();
    let compact_bytes = serde_json::to_vec(&state)
        .map_err(|error| format!("failed to compact criv state: {error}"))?
        .len();
    let architecture_compact_bytes = state
        .get("architecture")
        .map(serde_json::to_vec)
        .transpose()
        .map_err(|error| format!("failed to compact architecture state: {error}"))?
        .map_or(0, |value| value.len());
    let mut value_strings = StringInventory::default();
    let mut object_keys = StringInventory::default();
    collect_strings(&state, &mut value_strings, &mut object_keys);

    Ok(StateShape {
        state_bytes: bytes.len(),
        compact_bytes,
        architecture_compact_bytes,
        node_count: nodes.len(),
        edge_count: edges.len(),
        source_count,
        pattern_count,
        edge_endpoint_occurrences: endpoint_occurrences,
        edge_endpoint_bytes: endpoint_bytes,
        unique_edge_endpoint_count: unique_endpoints.len(),
        unique_edge_endpoint_bytes: unique_endpoint_bytes,
        repeated_edge_endpoint_bytes: endpoint_bytes.saturating_sub(unique_endpoint_bytes),
        value_strings: value_strings.shape(),
        object_keys: object_keys.shape(),
    })
}

fn collect_strings(
    value: &serde_json::Value,
    strings: &mut StringInventory,
    keys: &mut StringInventory,
) {
    match value {
        serde_json::Value::String(value) => strings.record(value),
        serde_json::Value::Array(values) => {
            for value in values {
                collect_strings(value, strings, keys);
            }
        }
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                keys.record(key);
                collect_strings(value, strings, keys);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: &str = r#"{
  "schema": "criv.state.v1",
  "architecture": {"model":{"name":"A"}},
  "graph": {
    "root": "",
    "nodes": [{"id":"n","hash":"h","kind":"code","label":"n","path":"src/a.rs"}],
    "edges": [{"from":"n","to":"n","kind":"self","hash":"e"}]
  },
  "registered-patterns": ["p"],
  "patterns": {"p":[]},
  "source-index": [{"path":"src/a.rs","mime":"text/rust","frecency":1}]
}"#;

    #[test]
    fn reports_graph_and_repeated_endpoint_shape() {
        let shape = analyze_state(STATE.as_bytes()).unwrap();

        assert_eq!(shape.node_count, 1);
        assert_eq!(shape.edge_count, 1);
        assert_eq!(shape.source_count, 1);
        assert_eq!(shape.pattern_count, 1);
        assert_eq!(shape.architecture_compact_bytes, 22);
        assert_eq!(shape.edge_endpoint_occurrences, 2);
        assert_eq!(shape.edge_endpoint_bytes, 2);
        assert_eq!(shape.unique_edge_endpoint_count, 1);
        assert_eq!(shape.unique_edge_endpoint_bytes, 1);
        assert_eq!(shape.repeated_edge_endpoint_bytes, 1);
    }

    #[test]
    fn rejects_an_unknown_state_schema() {
        let error = analyze_state(br#"{"schema":"criv.state.v2"}"#).unwrap_err();
        assert_eq!(error, "unsupported criv state schema: criv.state.v2");
    }

    #[test]
    fn reports_repeated_value_strings_separately_from_object_keys() {
        let state = br#"{
            "schema":"criv.state.v1",
            "graph":{"nodes":[],"edges":[]},
            "registered-patterns":[],
            "source-index":[{"path":"same"},{"path":"same"}]
        }"#;

        let shape = analyze_state(state).unwrap();

        assert_eq!(shape.value_strings.occurrences, 3);
        assert_eq!(shape.value_strings.bytes, 21);
        assert_eq!(shape.value_strings.unique_count, 2);
        assert_eq!(shape.value_strings.unique_bytes, 17);
        assert_eq!(shape.value_strings.repeated_bytes, 4);
        assert!(shape.object_keys.repeated_bytes >= 4);
    }

    #[test]
    fn summarizes_raw_load_samples_with_median_and_mad() {
        let summary = timing_summary(vec![1.0, 2.0, 100.0]).unwrap();

        assert_eq!(summary.samples, 3);
        assert_eq!(summary.raw_seconds, vec![1.0, 2.0, 100.0]);
        assert_eq!(summary.minimum_seconds, 1.0);
        assert_eq!(summary.median_seconds, 2.0);
        assert_eq!(summary.maximum_seconds, 100.0);
        assert_eq!(summary.median_absolute_deviation_seconds, 1.0);
    }
}
