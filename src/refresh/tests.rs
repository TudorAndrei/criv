use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use super::*;
use crate::util::copy_fixture_tree;
use crate::{policy_scan, source, structural, vault as vault_module};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct RefreshWork {
    policy_scan: policy_scan::WorkCounts,
    source_index: source::IndexWorkCounts,
    source_graph: source::GraphWorkCounts,
    vault: vault_module::WorkCounts,
    structural: structural::WorkCounts,
    state: state::WorkCounts,
}

#[derive(Debug, Eq, PartialEq)]
struct RefreshSnapshot {
    source_graph: source::SourceGraph,
    state_json: String,
    state_hash: String,
    latest: String,
    snapshot_json: String,
    diagnostics: Vec<check::Diagnostic>,
}

#[test]
fn refresh_does_not_create_architecture_source() {
    let temp = TempDir::new().unwrap();
    write_source_fixture(temp.path());
    state::reset_work_counts();
    let mut refresh = one_shot_session(temp.path());

    let result = refresh.refresh(temp.path(), RefreshCause::Initial).unwrap();

    let work = state::work_counts();
    assert_eq!(work.partitions_rebuilt, 2);
    assert_eq!(work.source_partitions_rebuilt, 1);
    assert_eq!(work.note_partitions_rebuilt, 0);
    assert_eq!(work.c4_partitions_rebuilt, 0);
    assert_eq!(work.policy_partitions_rebuilt, 0);
    assert_eq!(work.source_index_partitions_rebuilt, 1);
    assert_eq!(work.serializations, 1);
    assert!(result.vault().c4_artifacts.is_empty());
    assert!(!temp.path().join("docs/architecture/04-code.c4").exists());
}

#[test]
fn warm_one_shot_reuses_the_cached_source_graph() {
    let temp = TempDir::new().unwrap();
    write_source_fixture(temp.path());
    let mut cold = one_shot_session(temp.path());
    let cold = cold.refresh(temp.path(), RefreshCause::Initial).unwrap();
    assert_eq!(
        cold.vault().source_graph().changed_files(),
        &["src/lib.rs".to_string()]
    );

    let mut warm = one_shot_session(temp.path());
    let warm = warm.refresh(temp.path(), RefreshCause::Initial).unwrap();

    assert!(warm.vault().source_graph().changed_files().is_empty());
}

#[test]
fn live_refresh_reuses_source_catalog_for_docs_refresh() {
    let _live_test = source::lock_live_test();
    let fixture = incremental_fixture("one-live-adapter");
    source::reset_index_work_counts();
    let mut session = live_session(fixture.path());
    assert_eq!(source::index_work_counts().discovery_scans, 0);

    session
        .refresh(fixture.path(), RefreshCause::Initial)
        .unwrap();
    session
        .refresh(fixture.path(), RefreshCause::DocsChanged)
        .unwrap();

    assert_eq!(source::index_work_counts().discovery_scans, 1);
}

#[test]
fn one_shot_refresh_uses_one_full_source_observation() {
    let fixture = incremental_fixture("one-shot-source-catalog");
    let mut session = one_shot_session(fixture.path());
    reset_refresh_work();

    session
        .refresh(fixture.path(), RefreshCause::Initial)
        .unwrap();

    assert_source_catalog_work(refresh_work(), 1);
}

#[test]
fn live_refresh_rescans_source_after_a_content_event() {
    let _live_test = source::lock_live_test();
    let fixture = incremental_fixture("one-live-source-catalog");
    let root = fixture.path();
    let mut session = live_session(root);

    reset_refresh_work();
    session.refresh(root, RefreshCause::Initial).unwrap();
    let initial = refresh_work();
    assert_source_catalog_work(initial, 1);
    assert!(initial.source_graph.source_reads > 0);
    let source_reads = initial.source_graph.source_reads;

    reset_refresh_work();
    session.refresh(root, RefreshCause::DocsChanged).unwrap();
    let docs_changed = refresh_work();
    assert_source_catalog_work(docs_changed, 0);
    assert_eq!(docs_changed.source_graph.source_reads, 0);

    fs::write(root.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
    reset_refresh_work();
    session.refresh(root, RefreshCause::SourceChanged).unwrap();
    let source_changed = refresh_work();
    assert_source_catalog_work(source_changed, 1);
    assert_eq!(source_changed.source_graph.source_reads, source_reads);
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
    let mut session = one_shot_session(root);

    reset_refresh_work();
    session.refresh(root, RefreshCause::Initial).unwrap();

    assert_source_catalog_work(refresh_work(), 0);
}

#[test]
fn one_shot_refresh_reuses_one_compiled_policy_plan_for_state() {
    let fixture = incremental_fixture("shared-policy-plan");
    let mut session = one_shot_session(fixture.path());
    reset_refresh_work();

    session
        .refresh(fixture.path(), RefreshCause::Initial)
        .unwrap();

    assert_policy_refresh_work(refresh_work(), 1);
}

#[test]
fn live_refresh_reuses_one_policy_plan_for_no_op_and_changed_sources() {
    let _live_test = source::lock_live_test();
    let fixture = incremental_fixture("shared-live-policy-plan");
    let root = fixture.path();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn initial_change() {\n    println!(\"initial\");\n}\n",
    )
    .unwrap();
    let mut session = live_session(root);

    reset_refresh_work();
    session.refresh(root, RefreshCause::Initial).unwrap();
    assert_policy_refresh_work(refresh_work(), 1);

    reset_refresh_work();
    session.refresh(root, RefreshCause::DocsChanged).unwrap();
    assert_policy_refresh_work(refresh_work(), 0);

    fs::write(
        root.join("src/lib.rs"),
        "pub fn changed() {\n    println!(\"changed\");\n}\n",
    )
    .unwrap();
    reset_refresh_work();
    session.refresh(root, RefreshCause::SourceChanged).unwrap();
    assert_policy_refresh_work(refresh_work(), 1);
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
    let mut session = one_shot_session(root);

    reset_refresh_work();
    let error = session.refresh(root, RefreshCause::Initial).unwrap_err();
    let work = refresh_work();

    assert!(error.to_string().contains("state publication blocked"));
    assert_eq!(work.policy_scan, policy_scan::WorkCounts::default());
    assert_eq!(work.structural.policy_compilations, 0);
    assert_eq!(work.structural.ast_parses, 0);
}

#[test]
fn invalid_graph_cache_schema_converges_with_a_cache_free_build() {
    let incremental = incremental_fixture("invalid-schema-incremental");
    let full = incremental_fixture("invalid-schema-full");
    let mut initial = one_shot_session(incremental.path());
    initial
        .refresh(incremental.path(), RefreshCause::Initial)
        .unwrap();

    let cache_path = incremental.path().join(".criv/source-graph.json");
    let cache = fs::read_to_string(&cache_path).unwrap();
    fs::write(
        &cache_path,
        cache.replacen("criv.source-graph/3", "criv.source-graph/invalid", 1),
    )
    .unwrap();

    reset_refresh_work();
    let mut incremental_session = one_shot_session(incremental.path());
    let incremental_result = incremental_session
        .refresh(incremental.path(), RefreshCause::Initial)
        .unwrap();
    let work = refresh_work();
    let incremental_snapshot = refresh_snapshot(incremental.path(), incremental_result, None);
    let mut full_session = one_shot_session(full.path());
    let full_result = full_session
        .refresh(full.path(), RefreshCause::Initial)
        .unwrap();
    let full_snapshot = refresh_snapshot(full.path(), full_result, None);

    assert_refresh_eq(
        "invalid graph cache schema",
        &incremental_snapshot,
        &full_snapshot,
    );
    assert_eq!(work.source_graph.parsed_files, 2);
    assert_eq!(work.source_graph.cache_publications, 1);
}

#[test]
fn failed_source_refresh_forces_the_next_docs_refresh_to_rescan_source() {
    let _live_test = source::lock_live_test();
    let incremental = incremental_fixture("failed-refresh-incremental");
    let full = incremental_fixture("failed-refresh-full");
    let mut session = live_session(incremental.path());
    session
        .refresh(incremental.path(), RefreshCause::Initial)
        .unwrap();
    let before_hash = session.previous.as_ref().unwrap().state().hash().unwrap();
    let before_source = session
        .previous
        .as_ref()
        .unwrap()
        .vault()
        .source_graph()
        .clone();
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
            .refresh(incremental.path(), RefreshCause::SourceChanged)
            .is_err()
    );
    assert_eq!(
        session.previous.as_ref().unwrap().state().hash().unwrap(),
        before_hash
    );
    assert_eq!(
        session.previous.as_ref().unwrap().vault().source_graph(),
        &before_source
    );

    fs::remove_file(corrupt_snapshot).unwrap();
    fs::write(full.path().join("src/lib.rs"), "pub fn recovered() {}\n").unwrap();
    let previous = session.previous.as_ref().unwrap().state().clone();
    reset_refresh_work();
    let recovered = session
        .refresh(incremental.path(), RefreshCause::DocsChanged)
        .unwrap();
    assert!(refresh_work().source_graph.parsed_files > 0);
    let recovered = refresh_snapshot(incremental.path(), recovered, Some(&previous));
    let mut full_session = one_shot_session(full.path());
    let full_result = full_session
        .refresh(full.path(), RefreshCause::Initial)
        .unwrap();
    let full_snapshot = refresh_snapshot(full.path(), full_result, None);

    assert_refresh_eq("failed refresh retry", &recovered, &full_snapshot);
}

#[test]
fn unresolved_effective_governance_keeps_last_good_state_and_recovers() {
    let _live_test = source::lock_live_test();
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
    let mut session = live_session(root);
    session.refresh(root, RefreshCause::Initial).unwrap();
    let state_before = fs::read_to_string(root.join(".criv/state.json")).unwrap();
    let latest_before = fs::read_to_string(root.join(".criv/latest")).unwrap();
    let mut snapshots_before = fs::read_dir(root.join(".criv/snapshots"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    snapshots_before.sort();
    let previous_hash = session.previous.as_ref().unwrap().state().hash().unwrap();

    fs::remove_file(root.join("src/retired.rs")).unwrap();
    let error = session
        .refresh(root, RefreshCause::SourceChanged)
        .unwrap_err();

    assert!(error.to_string().contains("state publication blocked"));
    assert!(error.to_string().contains("src/retired.rs"));
    assert_eq!(
        session.previous.as_ref().unwrap().state().hash().unwrap(),
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
    let recovered = session.refresh(root, RefreshCause::DocsChanged).unwrap();
    let recovered_state: Value =
        serde_json::from_str(&fs::read_to_string(root.join(".criv/state.json")).unwrap()).unwrap();

    assert_ne!(recovered.state().hash().unwrap(), previous_hash);
    assert!(
        recovered_state["registered-patterns"]
            .as_array()
            .unwrap()
            .iter()
            .all(|pattern| pattern.as_str() != Some("ADR-0001/functions"))
    );
}

fn incremental_fixture(prefix: &str) -> TempDir {
    let temp = tempfile::Builder::new()
        .prefix(&format!("criv-refresh-{prefix}-"))
        .tempdir()
        .unwrap();
    copy_fixture_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/incremental-refresh"),
        temp.path(),
    )
    .unwrap();
    temp
}

fn one_shot_session(root: &Path) -> RefreshSession {
    RefreshSession::one_shot(root).unwrap()
}

fn live_session(root: &Path) -> RefreshSession {
    let config = Config::load(root).unwrap();
    RefreshSession::live(root, &config).unwrap()
}

fn reset_refresh_work() {
    policy_scan::reset_work_counts();
    source::reset_index_work_counts();
    source::reset_graph_work_counts();
    vault_module::reset_work_counts();
    structural::reset_work_counts();
    state::reset_work_counts();
}

fn refresh_work() -> RefreshWork {
    RefreshWork {
        policy_scan: policy_scan::work_counts(),
        source_index: source::index_work_counts(),
        source_graph: source::graph_work_counts(),
        vault: vault_module::work_counts(),
        structural: structural::work_counts(),
        state: state::work_counts(),
    }
}

fn assert_policy_refresh_work(work: RefreshWork, ast_parses: usize) {
    assert_eq!(
        work.policy_scan,
        policy_scan::WorkCounts {
            definition_compilations: 1,
            adr_scope_resolutions: 1,
        }
    );
    assert_eq!(work.structural.policy_compilations, 1);
    assert_eq!(work.structural.ast_parses, ast_parses);
}

fn assert_source_catalog_work(work: RefreshWork, materializations: usize) {
    assert_eq!(
        work.source_index,
        source::IndexWorkCounts {
            discovery_scans: materializations,
        }
    );
}

fn refresh_snapshot(
    root: &Path,
    result: &RefreshResult,
    previous_state: Option<&State>,
) -> RefreshSnapshot {
    let state_json = fs::read_to_string(root.join(".criv/state.json")).unwrap();
    assert_eq!(
        state_json,
        format!("{}\n", result.state().to_json().unwrap())
    );
    let latest = fs::read_to_string(root.join(".criv/latest")).unwrap();
    let snapshot_json = fs::read_to_string(
        root.join(".criv/snapshots")
            .join(format!("{}.json", latest.trim())),
    )
    .unwrap();
    assert_eq!(state_json, snapshot_json);

    RefreshSnapshot {
        source_graph: result.vault().source_graph().without_changed_files(),
        state_json,
        state_hash: result.state().hash().unwrap(),
        latest,
        snapshot_json,
        diagnostics: check::validate_with_previous_state(result.vault(), previous_state),
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
