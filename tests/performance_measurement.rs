use std::fs;
use std::path::Path;
use std::process::Output;

use assert_cmd::Command;
use tempfile::TempDir;

const MEASUREMENT_PATH: &str = ".criv/performance-measurement.json";

#[test]
fn instrumentation_preserves_success_semantics_and_artifacts() {
    let ordinary = fixture(false);
    let instrumented = fixture(false);

    let ordinary_output = run(ordinary.path(), &["watch", "--once"], false);
    let instrumented_output = run(instrumented.path(), &["watch", "--once"], true);

    assert_equivalent_output(&ordinary_output, &instrumented_output);
    assert!(!ordinary.path().join(MEASUREMENT_PATH).exists());
    assert_artifacts_equal(ordinary.path(), instrumented.path());
    let record = measurement(instrumented.path());
    assert_eq!(record["schema"], "criv.performance-measurement.v1");
    assert_eq!(record["success"], true);
    assert!(record["counters"]["notes_loaded"].as_u64().unwrap() >= 1);
    assert!(
        record["counters"]["source_graph_parsed_files"]
            .as_u64()
            .unwrap()
            >= 1
    );
    assert!(record["counters"]["state_serializations"].as_u64().unwrap() >= 1);
    assert!(
        record["spans"]["command.total"]["seconds"]
            .as_f64()
            .unwrap()
            >= 0.0
    );
}

#[test]
fn instrumentation_preserves_failure_semantics_and_artifacts() {
    let ordinary = fixture(true);
    let instrumented = fixture(true);

    let ordinary_output = run(ordinary.path(), &["check"], false);
    let instrumented_output = run(instrumented.path(), &["check"], true);

    assert!(!ordinary_output.status.success());
    assert_equivalent_output(&ordinary_output, &instrumented_output);
    assert!(!ordinary.path().join(MEASUREMENT_PATH).exists());
    assert_artifacts_equal(ordinary.path(), instrumented.path());
    let record = measurement(instrumented.path());
    assert_eq!(record["success"], false);
    assert!(record["counters"]["notes_loaded"].as_u64().unwrap() >= 1);
}

fn fixture(broken_link: bool) -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("docs/adr")).unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("criv.toml"),
        r#"[vault]
docs = "docs"
adr = "adr"

[source]
roots = ["src"]
exclude = []

[index]
source = true
embeddings = false
"#,
    )
    .unwrap();
    fs::write(
        root.path().join(".rumdl.toml"),
        "[global]\ndisable = [\"MD013\"]\n",
    )
    .unwrap();
    let body = if broken_link {
        "See [[missing-note]]."
    } else {
        "Stable content."
    };
    fs::write(
        root.path().join("docs/guide.md"),
        format!("---\nid: guide\nkind: doc\ntitle: Guide\n---\n\n# Guide\n\n{body}\n"),
    )
    .unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    )
    .unwrap();
    root
}

fn run(root: &Path, args: &[&str], instrumented: bool) -> Output {
    let mut command = Command::cargo_bin("criv").unwrap();
    command.current_dir(root).args(args);
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_PREFIX",
        "CRIV_PERF_MEASUREMENT_PATH",
    ] {
        command.env_remove(name);
    }
    if instrumented {
        command
            .env("CRIV_PERF_MEASUREMENT_PATH", MEASUREMENT_PATH)
            .env("CRIV_PERF_RUN_ID", "parity-run")
            .env("CRIV_PERF_SAMPLE_ID", "1")
            .env("CRIV_PERF_CASE", "parity")
            .env("CRIV_PERF_CACHE_STATE", "cold");
    }
    command.output().unwrap()
}

fn assert_equivalent_output(ordinary: &Output, instrumented: &Output) {
    assert_eq!(ordinary.status.code(), instrumented.status.code());
    assert_eq!(ordinary.stdout, instrumented.stdout);
    assert_eq!(ordinary.stderr, instrumented.stderr);
}

fn assert_artifacts_equal(ordinary: &Path, instrumented: &Path) {
    for relative in [
        ".criv/state.json",
        ".criv/latest",
        ".criv/source-graph.json",
    ] {
        let ordinary = fs::read(ordinary.join(relative)).ok();
        let instrumented = fs::read(instrumented.join(relative)).ok();
        assert_eq!(ordinary, instrumented, "artifact differs: {relative}");
    }
}

fn measurement(root: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(root.join(MEASUREMENT_PATH)).unwrap()).unwrap()
}
