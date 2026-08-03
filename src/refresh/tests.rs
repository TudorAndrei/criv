use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use super::*;
use crate::{source_graph, source_index, structural, vault as vault_module};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct RefreshWork {
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

#[derive(Debug, Clone, Copy)]
enum FixtureMutation {
    Noop,
    DocsProse,
    SourceEdit,
    SameSizeSourceEdit,
    AddSource,
    RenameSource,
    DeleteSource,
    PolicyDemotion,
    PolicyPromotionAndGovernance,
}

impl FixtureMutation {
    fn name(self) -> &'static str {
        match self {
            Self::Noop => "no-op",
            Self::DocsProse => "docs prose edit",
            Self::SourceEdit => "source edit",
            Self::SameSizeSourceEdit => "same-size source edit",
            Self::AddSource => "source add",
            Self::RenameSource => "source rename",
            Self::DeleteSource => "source delete",
            Self::PolicyDemotion => "policy demotion",
            Self::PolicyPromotionAndGovernance => "policy promotion and governance",
        }
    }

    fn cause(self) -> RefreshCause {
        if matches!(
            self,
            Self::DocsProse | Self::PolicyDemotion | Self::PolicyPromotionAndGovernance
        ) {
            RefreshCause::DocsChanged
        } else {
            RefreshCause::SourceChanged
        }
    }
}

#[test]
fn generated_code_architecture_is_included_in_the_same_refresh_state() {
    let temp = TempDir::new().unwrap();
    write_architecture_fixture(temp.path());
    state::reset_work_counts();
    let mut refresh = RefreshSession::one_shot(temp.path());

    let result = refresh.refresh(temp.path(), RefreshCause::Initial).unwrap();

    assert_eq!(state::work_counts().partitions_rebuilt, 1);
    assert_eq!(state::work_counts().serializations, 1);
    assert!(result.vault().resolve_note("architecture-code").is_some());
    let state: Value =
        serde_json::from_str(&fs::read_to_string(temp.path().join(".criv/state.json")).unwrap())
            .unwrap();
    assert!(
        state["graph"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["id"].as_str() == Some("note:architecture-code"))
    );
}

#[test]
fn warm_one_shot_reuses_the_cached_source_graph() {
    let temp = TempDir::new().unwrap();
    write_architecture_fixture(temp.path());
    let mut cold = RefreshSession::one_shot(temp.path());
    let cold = cold.refresh(temp.path(), RefreshCause::Initial).unwrap();
    assert_eq!(
        cold.vault().source_graph().changed_files(),
        &["src/lib.rs".to_string()]
    );

    let mut warm = RefreshSession::one_shot(temp.path());
    let warm = warm.refresh(temp.path(), RefreshCause::Initial).unwrap();

    assert!(warm.vault().source_graph().changed_files().is_empty());
}

#[test]
fn incremental_refresh_matches_a_cache_free_full_rebuild_after_each_mutation() {
    let incremental = incremental_fixture("incremental");
    let full = incremental_fixture("full");
    let mut incremental_session = RefreshSession::live(incremental.path(), None);
    let mut full_session = RefreshSession::one_shot(full.path());

    let incremental_result = incremental_session
        .refresh(incremental.path(), RefreshCause::Initial)
        .unwrap();
    let full_result = full_session
        .refresh(full.path(), RefreshCause::Initial)
        .unwrap();
    assert_refresh_eq(
        "cold build",
        &refresh_snapshot(incremental.path(), incremental_result, None),
        &refresh_snapshot(full.path(), full_result, None),
    );

    for mutation in [
        FixtureMutation::Noop,
        FixtureMutation::DocsProse,
        FixtureMutation::SourceEdit,
        FixtureMutation::SameSizeSourceEdit,
        FixtureMutation::AddSource,
        FixtureMutation::RenameSource,
        FixtureMutation::DeleteSource,
        FixtureMutation::PolicyDemotion,
        FixtureMutation::PolicyPromotionAndGovernance,
    ] {
        apply_fixture_mutation(incremental.path(), mutation);
        apply_fixture_mutation(full.path(), mutation);
        let incremental_previous = incremental_session
            .previous
            .as_ref()
            .map(|previous| previous.state().clone());
        let full_previous = full_session
            .previous
            .as_ref()
            .map(|previous| previous.state().clone());

        reset_refresh_work();
        let incremental_result = incremental_session
            .refresh(incremental.path(), mutation.cause())
            .unwrap();
        let work = refresh_work();
        let incremental_diagnostic_previous =
            matches!(mutation.cause(), RefreshCause::SourceChanged)
                .then_some(incremental_previous.as_ref().unwrap());
        let incremental_snapshot = refresh_snapshot(
            incremental.path(),
            incremental_result,
            incremental_diagnostic_previous,
        );

        fs::remove_file(full.path().join(".criv/source-graph.json")).unwrap();
        let mut next_full_session = RefreshSession::one_shot(full.path());
        let full_result = next_full_session
            .refresh(full.path(), RefreshCause::Initial)
            .unwrap();
        let full_diagnostic_previous = matches!(mutation.cause(), RefreshCause::SourceChanged)
            .then_some(full_previous.as_ref().unwrap());
        let full_snapshot = refresh_snapshot(full.path(), full_result, full_diagnostic_previous);

        assert_refresh_eq(mutation.name(), &incremental_snapshot, &full_snapshot);
        assert_final_work(mutation, work);
        full_session = next_full_session;
    }
}

#[test]
fn invalid_graph_cache_schema_converges_with_a_cache_free_build() {
    let incremental = incremental_fixture("invalid-schema-incremental");
    let full = incremental_fixture("invalid-schema-full");
    let mut initial = RefreshSession::one_shot(incremental.path());
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
    let mut incremental_session = RefreshSession::one_shot(incremental.path());
    let incremental_result = incremental_session
        .refresh(incremental.path(), RefreshCause::Initial)
        .unwrap();
    let work = refresh_work();
    let incremental_snapshot = refresh_snapshot(incremental.path(), incremental_result, None);
    let mut full_session = RefreshSession::one_shot(full.path());
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
    let incremental = incremental_fixture("failed-refresh-incremental");
    let full = incremental_fixture("failed-refresh-full");
    let mut session = RefreshSession::live(incremental.path(), None);
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
    let mut full_session = RefreshSession::one_shot(full.path());
    let full_result = full_session
        .refresh(full.path(), RefreshCause::Initial)
        .unwrap();
    let full_snapshot = refresh_snapshot(full.path(), full_result, None);

    assert_refresh_eq("failed refresh retry", &recovered, &full_snapshot);
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

fn apply_fixture_mutation(root: &Path, mutation: FixtureMutation) {
    match mutation {
        FixtureMutation::Noop => {}
        FixtureMutation::DocsProse => {
            let path = root.join("docs/guide.md");
            let mut contents = fs::read_to_string(&path).unwrap();
            contents.push_str("\nA prose-only refresh leaves source unchanged.\n");
            fs::write(path, contents).unwrap();
        }
        FixtureMutation::SourceEdit => {
            let path = root.join("src/lib.rs");
            let mut contents = fs::read_to_string(&path).unwrap();
            contents.push_str("\npub fn added() {}\n");
            fs::write(path, contents).unwrap();
        }
        FixtureMutation::SameSizeSourceEdit => {
            let path = root.join("src/lib.rs");
            let modified = fs::metadata(&path).unwrap().modified().unwrap();
            let contents = fs::read_to_string(&path)
                .unwrap()
                .replace("fn added", "fn other");
            assert_eq!(contents.len(), fs::metadata(&path).unwrap().len() as usize);
            fs::write(&path, contents).unwrap();
            fs::OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(modified))
                .unwrap();
        }
        FixtureMutation::AddSource => {
            fs::write(
                root.join("src/worker.py"),
                "def process(value: str) -> str:\n    return value.strip()\n",
            )
            .unwrap();
        }
        FixtureMutation::RenameSource => {
            fs::rename(root.join("src/helper.ts"), root.join("src/format.ts")).unwrap();
        }
        FixtureMutation::DeleteSource => {
            fs::remove_file(root.join("src/format.ts")).unwrap();
        }
        FixtureMutation::PolicyDemotion => {
            rewrite_policy(root, |contents| {
                contents.replace("status: accepted", "status: draft")
            });
        }
        FixtureMutation::PolicyPromotionAndGovernance => {
            rewrite_policy(root, |contents| {
                contents
                    .replace("status: draft", "status: accepted")
                    .replace("src/**/*.rs", "src/lib.rs")
            });
        }
    }
}

fn rewrite_policy(root: &Path, update: impl FnOnce(String) -> String) {
    let path = root.join("docs/adr/0001-no-println.md");
    let contents = fs::read_to_string(&path).unwrap();
    fs::write(path, update(contents)).unwrap();
}

fn reset_refresh_work() {
    source_index::reset_work_counts();
    source_graph::reset_work_counts();
    vault_module::reset_work_counts();
    structural::reset_work_counts();
    state::reset_work_counts();
}

fn refresh_work() -> RefreshWork {
    RefreshWork {
        source_index: source_index::work_counts(),
        source_graph: source_graph::work_counts(),
        vault: vault_module::work_counts(),
        structural: structural::work_counts(),
        state: state::work_counts(),
    }
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

fn assert_final_work(mutation: FixtureMutation, work: RefreshWork) {
    assert_eq!(work.state.partitions_rebuilt, 1);
    assert_eq!(work.state.serializations, 1);

    match mutation {
        FixtureMutation::Noop => {
            assert_eq!(work.source_graph.parsed_files, 0);
            assert_eq!(work.source_graph.reused_files, 2);
            assert_eq!(work.source_graph.cache_publications, 0);
            assert_eq!(work.structural.ast_parses, 0);
        }
        FixtureMutation::DocsProse => {
            assert_eq!(work.source_graph.parsed_files, 0);
            assert_eq!(work.source_graph.cache_publications, 0);
            assert_eq!(work.structural.ast_parses, 1);
        }
        FixtureMutation::SourceEdit | FixtureMutation::SameSizeSourceEdit => {
            assert_eq!(work.source_graph.parsed_files, 1);
            assert_eq!(work.source_graph.cache_publications, 1);
            assert_eq!(work.structural.ast_parses, 1);
        }
        FixtureMutation::AddSource | FixtureMutation::RenameSource => {
            assert_eq!(work.source_graph.parsed_files, 1);
            assert_eq!(work.source_graph.cache_publications, 1);
        }
        FixtureMutation::DeleteSource => {
            assert_eq!(work.source_graph.parsed_files, 0);
            assert_eq!(work.source_graph.cache_publications, 1);
        }
        FixtureMutation::PolicyDemotion | FixtureMutation::PolicyPromotionAndGovernance => {
            assert_eq!(work.source_graph.parsed_files, 0);
            assert_eq!(work.source_graph.cache_publications, 0);
        }
    }
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
output = "docs/architecture/04-code.md"
title = "Code diagram for criv"
"#,
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();
}
