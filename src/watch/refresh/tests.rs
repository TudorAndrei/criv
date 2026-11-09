use std::fs;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use serde_json::Value;
use tempfile::TempDir;

use super::*;
use crate::source;
use crate::test_support::copy_fixture_tree;
use crate::work::{self, Work};

static LIVE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn lock_live_test() -> MutexGuard<'static, ()> {
    LIVE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, Eq, PartialEq)]
struct RefreshSnapshot {
    source_files: Vec<source::SourceFile>,
    state_json: String,
    state_hash: String,
    latest: String,
    snapshot_json: String,
    diagnostics: Vec<crate::diagnostic::Diagnostic>,
}

#[test]
fn refresh_publishes_source_and_documentation_state() {
    let temp = TempDir::new().unwrap();
    write_source_fixture(temp.path());
    work::reset();
    let mut refresh = open_session(temp.path());

    refresh.refresh(RefreshCause::Initial, || Ok(())).unwrap();

    assert_eq!(
        work::counts([
            Work::SourcePartitionBuild,
            Work::NotePartitionBuild,
            Work::PolicyPartitionBuild,
            Work::StateSerialization,
        ]),
        [1, 0, 0, 1]
    );
    let state: Value =
        serde_json::from_str(&fs::read_to_string(temp.path().join(".criv/state.json")).unwrap())
            .unwrap();
    assert_eq!(state["source-index"].as_array().unwrap().len(), 1);
}

#[test]
fn warm_one_shot_reuses_the_cached_source_graph() {
    let temp = TempDir::new().unwrap();
    write_source_fixture(temp.path());
    let mut cold = open_session(temp.path());
    let cold = cold.refresh(RefreshCause::Initial, || Ok(())).unwrap();
    assert_eq!(
        cold.vault.source_graph().changed_files(),
        &["src/lib.rs".to_string()]
    );

    let mut warm = open_session(temp.path());
    let warm = warm.refresh(RefreshCause::Initial, || Ok(())).unwrap();

    assert!(warm.vault.source_graph().changed_files().is_empty());
}

#[test]
fn live_refresh_reuses_source_catalog_for_docs_refresh() {
    let _live_test = lock_live_test();
    let fixture = incremental_fixture("one-live-adapter");
    work::reset();
    let mut session = open_session(fixture.path());
    assert_eq!(work::count(Work::DiscoveryScan), 0);

    session.refresh(RefreshCause::Initial, || Ok(())).unwrap();
    session
        .refresh(RefreshCause::DocsChanged, || Ok(()))
        .unwrap();

    assert_eq!(work::count(Work::DiscoveryScan), 1);
}

#[test]
fn one_shot_refresh_uses_one_full_source_observation() {
    let fixture = incremental_fixture("one-shot-source-catalog");
    let mut session = open_session(fixture.path());
    work::reset();

    session.refresh(RefreshCause::Initial, || Ok(())).unwrap();

    assert_eq!(work::count(Work::DiscoveryScan), 1);
}

#[test]
fn live_refresh_rescans_source_after_a_content_event() {
    let _live_test = lock_live_test();
    let fixture = incremental_fixture("one-live-source-catalog");
    let root = fixture.path();
    let mut session = open_session(root);

    work::reset();
    session.refresh(RefreshCause::Initial, || Ok(())).unwrap();
    let [scans, source_reads] = work::counts([Work::DiscoveryScan, Work::SourceRead]);
    assert_eq!(scans, 1);
    assert!(source_reads > 0);

    work::reset();
    session
        .refresh(RefreshCause::DocsChanged, || Ok(()))
        .unwrap();
    assert_eq!(
        work::counts([Work::DiscoveryScan, Work::SourceRead]),
        [0, 0]
    );

    fs::write(root.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
    work::reset();
    session
        .refresh(RefreshCause::SourceChanged, || Ok(()))
        .unwrap();
    assert_eq!(
        work::counts([Work::DiscoveryScan, Work::SourceRead]),
        [1, source_reads]
    );
}

#[test]
fn disabled_source_refresh_materializes_no_source_catalog() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_source_fixture(root);
    fs::write(
        root.join("criv.toml"),
        "[source]\nroots = [\"src\"]\n\n[index]\nsource = false\n",
    )
    .unwrap();
    let mut session = open_session(root);

    work::reset();
    session.refresh(RefreshCause::Initial, || Ok(())).unwrap();

    assert_eq!(work::count(Work::DiscoveryScan), 0);
}

#[test]
fn one_shot_refresh_reuses_one_compiled_policy_plan_for_state() {
    let fixture = incremental_fixture("shared-policy-plan");
    let mut session = open_session(fixture.path());
    work::reset();

    session.refresh(RefreshCause::Initial, || Ok(())).unwrap();

    assert_policy_refresh_work(1);
}

#[test]
fn live_refresh_reuses_one_policy_plan_for_no_op_and_changed_sources() {
    let _live_test = lock_live_test();
    let fixture = incremental_fixture("shared-live-policy-plan");
    let root = fixture.path();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn initial_change() {\n    println!(\"initial\");\n}\n",
    )
    .unwrap();
    let mut session = open_session(root);

    work::reset();
    session.refresh(RefreshCause::Initial, || Ok(())).unwrap();
    assert_policy_refresh_work(1);

    work::reset();
    session
        .refresh(RefreshCause::DocsChanged, || Ok(()))
        .unwrap();
    assert_policy_refresh_work(0);

    fs::write(
        root.join("src/lib.rs"),
        "pub fn changed() {\n    println!(\"changed\");\n}\n",
    )
    .unwrap();
    work::reset();
    session
        .refresh(RefreshCause::SourceChanged, || Ok(()))
        .unwrap();
    assert_policy_refresh_work(1);
}

#[test]
fn disabled_source_refresh_stops_before_policy_planning() {
    let fixture = incremental_fixture("disabled-shared-policy-plan");
    let root = fixture.path();
    let config_path = root.join("criv.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace("source = true", "source = false"),
    )
    .unwrap();
    let mut session = open_session(root);

    work::reset();
    let error = session
        .refresh(RefreshCause::Initial, || Ok(()))
        .unwrap_err();

    assert!(error.to_string().contains("state publication blocked"));
    assert_eq!(
        work::counts([
            Work::PolicyDefinition,
            Work::PolicyScopeResolution,
            Work::PolicyCompilation,
            Work::AstParse,
        ]),
        [0; 4]
    );
}

#[test]
fn invalid_graph_cache_schema_converges_with_a_cache_free_build() {
    let incremental = incremental_fixture("invalid-schema-incremental");
    let full = incremental_fixture("invalid-schema-full");
    let mut initial = open_session(incremental.path());
    initial.refresh(RefreshCause::Initial, || Ok(())).unwrap();

    let cache_path = incremental.path().join(".criv/source-graph.json");
    let mut cache: Value = serde_json::from_str(&fs::read_to_string(&cache_path).unwrap()).unwrap();
    cache["schema"] = Value::from("criv.source-graph/invalid");
    fs::write(&cache_path, cache.to_string()).unwrap();

    work::reset();
    let mut incremental_session = open_session(incremental.path());
    let incremental_result = incremental_session
        .refresh(RefreshCause::Initial, || Ok(()))
        .unwrap();
    let parse_and_publish = work::counts([Work::SourceParse, Work::GraphCachePublication]);
    let incremental_snapshot = refresh_snapshot(incremental.path(), incremental_result);
    let mut full_session = open_session(full.path());
    let full_result = full_session
        .refresh(RefreshCause::Initial, || Ok(()))
        .unwrap();
    let full_snapshot = refresh_snapshot(full.path(), full_result);

    assert_refresh_eq(
        "invalid graph cache schema",
        &incremental_snapshot,
        &full_snapshot,
    );
    assert_eq!(parse_and_publish, [2, 1]);
}

#[test]
fn failed_source_refresh_forces_the_next_docs_refresh_to_rescan_source() {
    let _live_test = lock_live_test();
    let incremental = incremental_fixture("failed-refresh-incremental");
    let full = incremental_fixture("failed-refresh-full");
    let mut session = open_session(incremental.path());
    let before = session.refresh(RefreshCause::Initial, || Ok(())).unwrap();
    let before_hash = before.snapshot.clone();
    let before_source = before.vault.source_graph().clone();
    let corrupt_snapshot = incremental
        .path()
        .join(".criv/snapshots")
        .join(format!("{}.json", "a".repeat(64)));
    fs::write(&corrupt_snapshot, "{}\n").unwrap();
    fs::write(
        incremental.path().join("src/lib.rs"),
        "pub fn recovered() {}\n",
    )
    .unwrap();

    assert!(
        session
            .refresh(RefreshCause::SourceChanged, || Ok(()))
            .is_err()
    );
    assert_eq!(
        session.previous.as_ref().unwrap().snapshot.clone(),
        before_hash
    );
    assert_eq!(
        session.previous.as_ref().unwrap().vault.source_graph(),
        &before_source
    );

    fs::remove_file(corrupt_snapshot).unwrap();
    fs::write(full.path().join("src/lib.rs"), "pub fn recovered() {}\n").unwrap();
    work::reset();
    let recovered = session
        .refresh(RefreshCause::DocsChanged, || Ok(()))
        .unwrap();
    assert!(work::count(Work::SourceParse) > 0);
    let recovered = refresh_snapshot(incremental.path(), recovered);
    let mut full_session = open_session(full.path());
    let full_result = full_session
        .refresh(RefreshCause::Initial, || Ok(()))
        .unwrap();
    let full_snapshot = refresh_snapshot(full.path(), full_result);

    assert_refresh_eq("failed refresh retry", &recovered, &full_snapshot);
}

#[test]
fn unresolved_effective_governance_keeps_last_good_state_and_recovers() {
    let _live_test = lock_live_test();
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    fs::write(
        root.join("criv.toml"),
        r#"[source]
roots = ["src"]
"#,
    )
    .unwrap();
    fs::write(root.join("src/retired.rs"), "fn retired() {}\n").unwrap();
    fs::write(root.join("src/current.rs"), "fn current() {}\n").unwrap();
    fs::write(
        root.join("docs/adr/0001-retired.md"),
        r#"---
id: ADR-0001
kind: decision
title: Retired implementation
status: accepted
governs:
  - src/retired.rs
policy:
  patterns:
    - id: functions
      language: rust
      pattern: "fn $NAME() { $$$ }"
---

# Retired implementation
"#,
    )
    .unwrap();
    let mut session = open_session(root);
    let previous_hash = session
        .refresh(RefreshCause::Initial, || Ok(()))
        .unwrap()
        .snapshot
        .clone();
    let state_before = fs::read_to_string(root.join(".criv/state.json")).unwrap();
    let latest_before = fs::read_to_string(root.join(".criv/latest")).unwrap();
    let mut snapshots_before = fs::read_dir(root.join(".criv/snapshots"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    snapshots_before.sort();

    fs::remove_file(root.join("src/retired.rs")).unwrap();
    let error = session
        .refresh(RefreshCause::SourceChanged, || Ok(()))
        .unwrap_err();

    assert!(error.to_string().contains("state publication blocked"));
    assert!(error.to_string().contains("src/retired.rs"));
    assert_eq!(
        session.previous.as_ref().unwrap().snapshot.clone(),
        previous_hash
    );
    assert_eq!(
        fs::read_to_string(root.join(".criv/state.json")).unwrap(),
        state_before
    );
    assert_eq!(
        fs::read_to_string(root.join(".criv/latest")).unwrap(),
        latest_before
    );
    let mut snapshots_after = fs::read_dir(root.join(".criv/snapshots"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    snapshots_after.sort();
    assert_eq!(snapshots_after, snapshots_before);

    fs::write(
        root.join("docs/adr/0002-successor.md"),
        r#"---
id: ADR-0002
kind: decision
title: Remove retired implementation
status: accepted
supersedes:
  - ADR-0001
governs:
  - src/current.rs
---

# Remove retired implementation
"#,
    )
    .unwrap();
    let recovered = session
        .refresh(RefreshCause::DocsChanged, || Ok(()))
        .unwrap();
    let recovered_state: Value =
        serde_json::from_str(&fs::read_to_string(root.join(".criv/state.json")).unwrap()).unwrap();

    assert_ne!(recovered.snapshot.clone(), previous_hash);
    assert!(
        recovered_state["registered-patterns"]
            .as_array()
            .unwrap()
            .iter()
            .all(|pattern| pattern.as_str() != Some("ADR-0001/functions"))
    );
}

#[test]
fn changing_a_file_governed_through_a_selector_rescans_its_policy() {
    let _live_test = lock_live_test();
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_source_fixture(root);
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "fn run() {\n    println!(\"before\");\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/adr/0001-selector.md"),
        r#"---
id: ADR-0001
kind: decision
title: Selector governance
status: accepted
governs:
  - src/lib.rs#fn:run
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: println!($$$ARGS)
---

# Selector governance
"#,
    )
    .unwrap();
    let mut session = open_session(root);
    session.refresh(RefreshCause::Initial, || Ok(())).unwrap();
    assert_eq!(policy_match_count(root, "ADR-0001/no-println"), 1);

    fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();
    session
        .refresh(RefreshCause::SourceChanged, || Ok(()))
        .unwrap();

    assert_eq!(policy_match_count(root, "ADR-0001/no-println"), 0);
}

fn policy_match_count(root: &Path, pattern_id: &str) -> usize {
    let state: Value =
        serde_json::from_str(&fs::read_to_string(root.join(".criv/state.json")).unwrap()).unwrap();
    state["patterns"][pattern_id].as_array().unwrap().len()
}

fn incremental_fixture(prefix: &str) -> TempDir {
    let temp = tempfile::Builder::new()
        .prefix(&format!("criv-refresh-{prefix}-"))
        .tempdir()
        .unwrap();
    copy_fixture_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/incremental-refresh"),
        temp.path(),
    );
    temp
}

fn open_session(root: &Path) -> RefreshSession {
    let files = RepositoryFiles::open(root).unwrap();
    RefreshSession::new(&files, &Config::load_from(&files).unwrap())
}

fn assert_policy_refresh_work(ast_parses: usize) {
    assert_eq!(
        work::counts([
            Work::PolicyDefinition,
            Work::PolicyScopeResolution,
            Work::PolicyCompilation,
            Work::AstParse,
        ]),
        [1, 1, 1, ast_parses]
    );
}

fn refresh_snapshot(root: &Path, result: &RefreshResult) -> RefreshSnapshot {
    let state_json = fs::read_to_string(root.join(".criv/state.json")).unwrap();
    let latest = fs::read_to_string(root.join(".criv/latest")).unwrap();
    assert_eq!(latest.trim(), result.snapshot);
    let snapshot_json = fs::read_to_string(
        root.join(".criv/snapshots")
            .join(format!("{}.json", latest.trim())),
    )
    .unwrap();
    assert_eq!(state_json, snapshot_json);

    RefreshSnapshot {
        source_files: result.vault.source_graph().files().cloned().collect(),
        state_json,
        state_hash: result.snapshot.clone(),
        latest,
        snapshot_json,
        diagnostics: check::validate_vault(&result.vault).diagnostics,
    }
}

fn assert_refresh_eq(name: &str, incremental: &RefreshSnapshot, full: &RefreshSnapshot) {
    assert_eq!(
        incremental, full,
        "{name} diverged from a cache-free full rebuild"
    );
}

fn write_source_fixture(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("criv.toml"),
        r#"
[source]
roots = ["src"]
"#,
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();
}
