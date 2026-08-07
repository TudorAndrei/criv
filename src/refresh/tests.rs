use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

use super::*;
use crate::{policy_scan, source_graph, source_index, structural, vault as vault_module};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct RefreshWork {
    policy_scan: policy_scan::WorkCounts,
    source_index: source_index::WorkCounts,
    source_graph: source_graph::WorkCounts,
    vault: vault_module::WorkCounts,
    structural: structural::WorkCounts,
    state: state::WorkCounts,
}

#[derive(Debug, Eq, PartialEq)]
struct RefreshSnapshot {
    source_graph: source_graph::SourceGraph,
    state_json: String,
    state_hash: String,
    latest: String,
    snapshot_json: String,
    generated_architecture: String,
    diagnostics: Vec<check::Diagnostic>,
}

#[test]
fn generated_code_architecture_is_included_in_the_same_refresh_state() {
    let temp = TempDir::new().unwrap();
    write_architecture_fixture(temp.path());
    state::reset_work_counts();
    let mut refresh = one_shot_session(temp.path());

    let result = refresh.refresh(temp.path(), RefreshCause::Initial).unwrap();

    let work = state::work_counts();
    assert_eq!(work.partitions_rebuilt, 3);
    assert_eq!(work.source_partitions_rebuilt, 1);
    assert_eq!(work.note_partitions_rebuilt, 0);
    assert_eq!(work.c4_partitions_rebuilt, 1);
    assert_eq!(work.policy_partitions_rebuilt, 0);
    assert_eq!(work.source_index_partitions_rebuilt, 1);
    assert_eq!(work.serializations, 1);
    assert!(
        result
            .vault()
            .c4_artifacts
            .iter()
            .any(|artifact| artifact.rel_path == "docs/architecture/04-code.c4")
    );
    let state: Value =
        serde_json::from_str(&fs::read_to_string(temp.path().join(".criv/state.json")).unwrap())
            .unwrap();
    assert!(
        state["graph"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| {
                node["id"].as_str() == Some("architecture-source:docs/architecture/04-code.c4")
            })
    );
}

#[test]
fn warm_one_shot_reuses_the_cached_source_graph() {
    let temp = TempDir::new().unwrap();
    write_architecture_fixture(temp.path());
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
fn live_refresh_reuses_exactly_one_source_index_adapter() {
    let _live_test = source_index::lock_live_test();
    let fixture = incremental_fixture("one-live-adapter");
    source_index::reset_work_counts();
    let mut session = live_session(fixture.path());
    assert_eq!(source_index::work_counts().fff_starts, 1);

    session
        .refresh(fixture.path(), RefreshCause::Initial)
        .unwrap();
    session
        .refresh(fixture.path(), RefreshCause::DocsChanged)
        .unwrap();

    assert_eq!(source_index::work_counts().fff_starts, 1);
}

#[test]
fn one_shot_refresh_materializes_one_source_catalog() {
    let fixture = incremental_fixture("one-shot-source-catalog");
    let mut session = one_shot_session(fixture.path());
    reset_refresh_work();

    session
        .refresh(fixture.path(), RefreshCause::Initial)
        .unwrap();

    assert_source_catalog_work(refresh_work(), 1);
}

#[test]
fn live_refresh_materializes_one_source_catalog_for_initial_no_op_and_change() {
    let _live_test = source_index::lock_live_test();
    let fixture = incremental_fixture("one-live-source-catalog");
    let root = fixture.path();
    let mut session = live_session(root);

    reset_refresh_work();
    session.refresh(root, RefreshCause::Initial).unwrap();
    assert_source_catalog_work(refresh_work(), 1);

    reset_refresh_work();
    session.refresh(root, RefreshCause::DocsChanged).unwrap();
    assert_source_catalog_work(refresh_work(), 1);

    fs::write(root.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
    wait_for_source_change(&mut session, "source catalog change");
    reset_refresh_work();
    session.refresh(root, RefreshCause::SourceChanged).unwrap();
    assert_source_catalog_work(refresh_work(), 1);
}

#[test]
fn disabled_source_refresh_materializes_no_source_catalog() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_architecture_fixture(root);
    let config_path = root.join("criv.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace(
            "[architecture.code]",
            "[index]\nsource = false\n\n[architecture.code]",
        ),
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
    let _live_test = source_index::lock_live_test();
    let fixture = incremental_fixture("shared-live-policy-plan");
    let root = fixture.path();
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
    wait_for_source_change(&mut session, "shared policy plan source change");
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
        cache.replacen("criv.source-graph/2", "criv.source-graph/invalid", 1),
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
fn failed_refresh_retries_from_the_last_successful_state() {
    let _live_test = source_index::lock_live_test();
    let incremental = incremental_fixture("failed-refresh-incremental");
    let full = incremental_fixture("failed-refresh-full");
    let mut session = live_session(incremental.path());
    session
        .refresh(incremental.path(), RefreshCause::Initial)
        .unwrap();
    let before_hash = session.previous.as_ref().unwrap().state().hash().unwrap();
    let config_path = incremental.path().join("criv.toml");
    let valid_config = fs::read_to_string(&config_path).unwrap();
    fs::write(&config_path, "[vault]\ndocs = \"/outside\"\n").unwrap();

    assert!(
        session
            .refresh(incremental.path(), RefreshCause::DocsChanged)
            .is_err()
    );
    assert_eq!(
        session.previous.as_ref().unwrap().state().hash().unwrap(),
        before_hash
    );

    fs::write(&config_path, valid_config).unwrap();
    fs::write(
        incremental.path().join("src/lib.rs"),
        "pub fn recovered() {}\n",
    )
    .unwrap();
    fs::write(full.path().join("src/lib.rs"), "pub fn recovered() {}\n").unwrap();
    let previous = session.previous.as_ref().unwrap().state().clone();
    let recovered = session
        .refresh(incremental.path(), RefreshCause::SourceChanged)
        .unwrap();
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
    let _live_test = source_index::lock_live_test();
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    fs::create_dir_all(root.join("docs/architecture")).unwrap();
    fs::write(
        root.join("criv.toml"),
        r#"[source]
roots = ["src"]

[architecture.code]
output = "docs/architecture/04-code.c4"
title = "Code"
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
    let architecture_before =
        fs::read_to_string(root.join("docs/architecture/04-code.c4")).unwrap();
    let mut snapshots_before = fs::read_dir(root.join(".criv/snapshots"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    snapshots_before.sort();
    let previous_hash = session.previous.as_ref().unwrap().state().hash().unwrap();

    fs::remove_file(root.join("src/retired.rs")).unwrap();
    wait_for_source_change(&mut session, "governed source deletion");
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
    assert_eq!(
        fs::read_to_string(root.join("docs/architecture/04-code.c4")).unwrap(),
        architecture_before
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
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/incremental-refresh"),
        temp.path(),
    );
    temp
}

fn one_shot_session(root: &Path) -> RefreshSession {
    RefreshSession::one_shot(root).unwrap()
}

fn live_session(root: &Path) -> RefreshSession {
    let config = Config::load(root).unwrap();
    RefreshSession::live(root, &config).unwrap()
}

fn wait_for_source_change(session: &mut RefreshSession, label: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match session.observe_source_change().unwrap() {
            source_index::SourceChange::Changed => return,
            source_index::SourceChange::Unchanged => {}
            source_index::SourceChange::Disabled => panic!("{label} used a disabled source index"),
        }
        assert!(
            Instant::now() < deadline,
            "timed out observing {label} in the live source catalog"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn copy_fixture_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_fixture_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn reset_refresh_work() {
    policy_scan::reset_work_counts();
    source_index::reset_work_counts();
    source_graph::reset_work_counts();
    vault_module::reset_work_counts();
    structural::reset_work_counts();
    state::reset_work_counts();
}

fn refresh_work() -> RefreshWork {
    RefreshWork {
        policy_scan: policy_scan::work_counts(),
        source_index: source_index::work_counts(),
        source_graph: source_graph::work_counts(),
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
        source_index::WorkCounts {
            fff_starts: 0,
            catalog_traversals: materializations,
            source_enumerations: materializations,
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
        generated_architecture: fs::read_to_string(
            root.join("docs/architecture/04-generated-code.c4"),
        )
        .unwrap(),
        diagnostics: check::validate_with_previous_state(result.vault(), previous_state),
    }
}

fn assert_refresh_eq(name: &str, incremental: &RefreshSnapshot, full: &RefreshSnapshot) {
    assert_eq!(
        incremental, full,
        "{name} diverged from a cache-free full rebuild"
    );
}

fn write_architecture_fixture(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("criv.toml"),
        r#"
[source]
roots = ["src"]

[architecture.code]
output = "docs/architecture/04-code.c4"
title = "Code diagram for criv"
"#,
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();
}
