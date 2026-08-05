use std::fs;

use assert_cmd::Command;

const CURRENT: &str = r#"{
  "schema": "criv.state.v1",
  "architecture": {"model":{"name":"A"}},
  "graph": {
    "root": "root-a",
    "nodes": [{"id":"code:src/lib.rs","hash":"h1","kind":"code","label":"lib","path":"src/lib.rs"}],
    "edges": []
  },
  "registered-patterns": [],
  "patterns": {},
  "source-index": [{"path":"src/lib.rs","mime":"text/rust","frecency":1}]
}"#;

const CHANGED: &str = r#"{
  "schema": "criv.state.v1",
  "architecture": {"model":{"name":"A"}},
  "graph": {
    "root": "root-b",
    "nodes": [
      {"id":"code:src/lib.rs","hash":"h2","kind":"code","label":"lib","path":"src/lib.rs"},
      {"id":"symbol:src/lib.rs#fn:new_symbol","hash":"h3","kind":"function","label":"new_symbol","path":"src/lib.rs#L2"}
    ],
    "edges": []
  },
  "registered-patterns": [],
  "patterns": {},
  "source-index": [{"path":"src/lib.rs","mime":"text/rust","frecency":1}]
}"#;

#[test]
fn public_cli_reports_json_baseline_correctness() {
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current.json");
    let changed = root.path().join("changed.json");
    fs::write(&current, CURRENT).unwrap();
    fs::write(&changed, CHANGED).unwrap();

    let output = Command::cargo_bin("criv-state-store-bench")
        .unwrap()
        .args([
            "--candidate",
            "json",
            "--state",
            current.to_str().unwrap(),
            "--changed-state",
            changed.to_str().unwrap(),
            "--samples",
            "1",
            "--allow-low-samples",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "criv.state-store-candidate.v1");
    assert_eq!(report["candidate"], "json");
    assert_eq!(report["samples"], 1);
    assert_eq!(
        report["evidence"]["state_digest"].as_str().unwrap().len(),
        64
    );
    assert!(report["evidence"]["profile"].as_str().is_some());
    assert_eq!(report["correctness"]["deterministic_bytes"], true);
    assert_eq!(report["correctness"]["logical_round_trip"], true);
    assert_eq!(report["correctness"]["rejects_truncated"], true);
    assert_eq!(report["correctness"]["rejects_corrupt"], true);
    assert_eq!(report["correctness"]["rejects_unknown_version"], true);
    assert_eq!(
        report["correctness"]["interrupted_publication_keeps_current"],
        true
    );
}

#[test]
fn public_cli_reports_postcard_full_decode_control() {
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current.json");
    let changed = root.path().join("changed.json");
    fs::write(&current, CURRENT).unwrap();
    fs::write(&changed, CHANGED).unwrap();

    let output = Command::cargo_bin("criv-state-store-bench")
        .unwrap()
        .args([
            "--candidate",
            "postcard",
            "--state",
            current.to_str().unwrap(),
            "--changed-state",
            changed.to_str().unwrap(),
            "--samples",
            "1",
            "--allow-low-samples",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["candidate"], "postcard");
    assert_eq!(report["layout"], "full-decode-control");
    for result in report["correctness"].as_object().unwrap().values() {
        assert_eq!(result, true);
    }
}

#[test]
fn public_cli_reports_checked_rkyv_archive() {
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current.json");
    let changed = root.path().join("changed.json");
    fs::write(&current, CURRENT).unwrap();
    fs::write(&changed, CHANGED).unwrap();

    let output = Command::cargo_bin("criv-state-store-bench")
        .unwrap()
        .args([
            "--candidate",
            "rkyv",
            "--state",
            current.to_str().unwrap(),
            "--changed-state",
            changed.to_str().unwrap(),
            "--samples",
            "1",
            "--allow-low-samples",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["candidate"], "rkyv");
    assert_eq!(report["layout"], "partitioned-checked-archive-upper-bound");
    assert!(report["storage"]["partition_count"].as_u64().unwrap() > 1);
    assert!(
        report["storage"]["changed_partition_count"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        report["storage"]["reused_partition_count"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        report["storage"]["retained_snapshot_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(report["storage"]["edge_endpoint_width_bits"], 32);
    for result in report["correctness"].as_object().unwrap().values() {
        assert_eq!(result, true);
    }
}

#[test]
fn public_cli_reports_partitioned_criv_columns() {
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current.json");
    let changed = root.path().join("changed.json");
    fs::write(&current, CURRENT).unwrap();
    fs::write(&changed, CHANGED).unwrap();

    let output = Command::cargo_bin("criv-state-store-bench")
        .unwrap()
        .args([
            "--candidate",
            "criv-column",
            "--state",
            current.to_str().unwrap(),
            "--changed-state",
            changed.to_str().unwrap(),
            "--samples",
            "1",
            "--allow-low-samples",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["candidate"], "criv-column");
    assert_eq!(report["layout"], "partitioned-column-store");
    assert_eq!(report["storage"]["edge_endpoint_width_bits"], 32);
    assert_eq!(
        report["storage"]["publication_model"],
        "content-addressed-partitions"
    );
    assert!(report["storage"]["partition_count"].as_u64().unwrap() > 1);
    assert!(
        report["storage"]["changed_publication_bytes"]
            .as_u64()
            .unwrap()
            < report["storage"]["stored_bytes"].as_u64().unwrap() * 2
    );
    for result in report["correctness"].as_object().unwrap().values() {
        assert_eq!(result, true);
    }
}

#[test]
fn public_cli_reports_partitioned_flatbuffers() {
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current.json");
    let changed = root.path().join("changed.json");
    fs::write(&current, CURRENT).unwrap();
    fs::write(&changed, CHANGED).unwrap();

    let output = Command::cargo_bin("criv-state-store-bench")
        .unwrap()
        .args([
            "--candidate",
            "flatbuffers",
            "--state",
            current.to_str().unwrap(),
            "--changed-state",
            changed.to_str().unwrap(),
            "--samples",
            "1",
            "--allow-low-samples",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["candidate"], "flatbuffers");
    assert_eq!(report["layout"], "partitioned-flatbuffers");
    assert_eq!(report["storage"]["edge_endpoint_width_bits"], 32);
    assert!(report["storage"]["partition_count"].as_u64().unwrap() > 1);
    for result in report["correctness"].as_object().unwrap().values() {
        assert_eq!(result, true);
    }
}

#[test]
fn public_cli_measures_required_native_operations() {
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current.json");
    let changed = root.path().join("changed.json");
    fs::write(&current, CURRENT).unwrap();
    fs::write(&changed, CHANGED).unwrap();

    let output = Command::cargo_bin("criv-state-store-bench")
        .unwrap()
        .args([
            "--candidate",
            "criv-column",
            "--state",
            current.to_str().unwrap(),
            "--changed-state",
            changed.to_str().unwrap(),
            "--samples",
            "1",
            "--allow-low-samples",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["native"]["load_validate_peak_rss_bytes"]
            .as_u64()
            .is_some()
    );
    let evidence = &report["native"]["evidence"];
    assert_eq!(evidence["node_count"], 1);
    assert_eq!(evidence["edge_count"], 0);
    assert_eq!(evidence["source_count"], 1);
    assert_eq!(evidence["lookup_present"], true);
    assert_eq!(evidence["lookup_missing"], false);
    assert_eq!(evidence["exact_selector_target"], "src/lib.rs");
    assert_eq!(evidence["nodes_added"], 1);
    assert_eq!(evidence["nodes_changed"], 1);
    for operation in report["native"]["operations"].as_object().unwrap().values() {
        assert_eq!(operation["samples"], 1);
        assert!(operation["median_seconds"].as_f64().unwrap() >= 0.0);
    }
}

#[test]
fn public_cli_includes_packaged_wasm_measurements() {
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current.json");
    let changed = root.path().join("changed.json");
    fs::write(&current, CURRENT).unwrap();
    fs::write(&changed, CHANGED).unwrap();
    let package = root.path().join("pkg");
    fs::create_dir(&package).unwrap();
    fs::write(
        package.join("package.json"),
        r#"{"main":"state_store.js","type":"commonjs"}"#,
    )
    .unwrap();
    fs::write(
        package.join("state_store.js"),
        r#"module.exports = {
          CandidateStore: class {
            constructor(candidate, bytes) { this.candidate = candidate; this.bytes = bytes; }
            validatedState() { return { candidate: this.candidate }; }
            summary() { return { node_count: 1 }; }
            sourceEntries() { return [{ path: "src/lib.rs" }]; }
            graphNodes() { return [{ id: "code:src/lib.rs" }]; }
            lookup(target) { return target === "code:src/lib.rs"; }
            selectors(query) { return query === "missing" ? [] : ["src/lib.rs"]; }
          }
        };"#,
    )
    .unwrap();
    fs::write(package.join("state_store_bg.wasm"), [0, 1, 2, 3]).unwrap();

    let output = Command::cargo_bin("criv-state-store-bench")
        .unwrap()
        .args([
            "--candidate",
            "criv-column",
            "--state",
            current.to_str().unwrap(),
            "--changed-state",
            changed.to_str().unwrap(),
            "--wasm-package",
            package.to_str().unwrap(),
            "--samples",
            "1",
            "--allow-low-samples",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["wasm"]["schema"],
        "criv.state-store-wasm-candidate.v1"
    );
    assert_eq!(report["wasm"]["wasm_module_bytes"], 4);
    assert_eq!(
        report["wasm"]["operations"]["initial_projections_after_load"]["timing"]["samples"],
        1
    );
    assert!(
        report["wasm"]["operations"]["lookup_present"]["peak_rss"]["median"]
            .as_u64()
            .is_some()
    );
}
