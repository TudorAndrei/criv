use criv_state_wire::{AssetIndexEntry, SourceIndexEntry, StateDocument, content_hash};
use serde::{Serialize, Serializer};

use crate::policy_scan::{PolicyFailure, PolicyScanPlan};
use crate::source::Language;
use crate::vault::Vault;
#[cfg(test)]
use crate::work::{self, Work};
use crate::{CrivError, Result};

mod partitions;
mod projection;
mod publication;
mod snapshots;
mod stable_hash;

use partitions::StatePartitions;
pub use publication::load_snapshot;
pub use snapshots::SnapshotId;

#[derive(Debug, Clone)]
pub struct State {
    wire: StateDocument,
    partitions: StatePartitions,
}

struct SerializedState {
    published: String,
    hash: String,
}

impl State {
    fn build(vault: &Vault, previous: Option<&Self>, policies: &PolicyScanPlan) -> Result<Self> {
        let failures = policies.registered_failures();
        if !failures.is_empty() {
            return Err(CrivError::new(
                failures
                    .iter()
                    .map(PolicyFailure::message)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ));
        }
        let partitions =
            StatePartitions::build(vault, previous.map(|state| &state.partitions), policies)?;
        let patterns = partitions.patterns();
        let registered_patterns = patterns.keys().cloned().collect();
        let source_index = vault
            .source_files()
            .iter()
            .map(|path| SourceIndexEntry {
                mime: source_mime(path),
                path: path.clone(),
            })
            .collect();
        let mut wire = StateDocument::new(
            partitions.graph(),
            registered_patterns,
            patterns,
            source_index,
        );
        wire.asset_index = vault
            .documentation_assets()
            .iter()
            .map(|asset| AssetIndexEntry {
                path: asset.path.clone(),
                mime: asset.mime.clone(),
                bytes: asset.bytes,
                hash: asset.hash.clone(),
            })
            .collect();
        Ok(Self { wire, partitions })
    }

    fn serialize(&self) -> Result<SerializedState> {
        #[cfg(test)]
        work::record(Work::StateSerialization);

        let json = serde_json::to_string_pretty(&self.wire)
            .map_err(|err| CrivError::new(format!("failed to serialize state: {err}")))?;
        Ok(SerializedState {
            hash: content_hash::of_document(&json),
            published: format!("{json}\n"),
        })
    }
}

/// Builds State from `vault`, reusing the partitions of `previous` that the
/// Source files the vault observed as changed and the vault catalogs leave
/// valid, and publishes it once `precommit_check` passes. Returns the
/// snapshot hash and the State.
pub fn write_state(
    vault: &Vault,
    previous: Option<&State>,
    policies: &PolicyScanPlan,
    precommit_check: impl FnOnce() -> Result<()>,
) -> Result<(String, State)> {
    let state = State::build(vault, previous, policies)?;
    let serialized = state.serialize()?;
    publication::publish(
        vault.repository_files(),
        &serialized.hash,
        &serialized.published,
        vault.config().state_keep(),
        precommit_check,
    )?;
    Ok((serialized.hash, state))
}

impl Serialize for State {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.wire.serialize(serializer)
    }
}

fn source_mime(path: &str) -> Option<String> {
    if let Some(mime) = Language::from_path(path).mime() {
        return Some(mime.into());
    }
    mime_guess::from_path(path).first_raw().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use criv_state_wire::{EdgeKind, STATE_SCHEMA};

    use super::*;
    use crate::repository::RepositoryFiles;
    use crate::vault::LoadOptions;

    fn load(root: &Path) -> Result<Vault> {
        Vault::load(&RepositoryFiles::open(root)?, LoadOptions::default())
    }

    fn build(vault: &Vault) -> Result<State> {
        build_incremental(vault, None)
    }

    /// Builds over `previous` from a vault whose Source changes came from a
    /// reload against the published Source graph cache.
    fn build_incremental(vault: &Vault, previous: Option<&State>) -> Result<State> {
        State::build(vault, previous, &PolicyScanPlan::new(vault))
    }

    fn publish(vault: &Vault) -> Result<(String, State)> {
        publish_incremental(vault, None)
    }

    fn publish_incremental(vault: &Vault, previous: Option<&State>) -> Result<(String, State)> {
        write_state(vault, previous, &PolicyScanPlan::new(vault), || Ok(()))
    }

    #[test]
    fn state_projects_the_vault_link_resolutions_without_resolving_again() {
        let root = tempfile::TempDir::new().unwrap();
        let root = root.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::write(root.join("criv.toml"), "[source]\nroots = [\"src\"]\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();
        std::fs::write(root.join("src/helper.rs"), "fn help() {}\n").unwrap();
        std::fs::write(
            root.join("docs/lib.rs.md"),
            "---\nid: collision-note\nkind: doc\ntitle: Collision note\n---\n\n# Collision note\n",
        )
        .unwrap();
        std::fs::write(
            root.join("docs/adr/0001-local-cli-vault-architecture.md"),
            r#"---
id: ADR-0001
kind: decision
title: Local CLI vault architecture
status: accepted
policy:
  patterns:
    - id: no-block-on
      language: rust
      pattern: "println!($$$ARGS)"
---

# Local CLI vault architecture

See [[lib.rs]], [[match:ADR-0001/no-block-on]], and [[helper.rs#help]].
"#,
        )
        .unwrap();
        let vault = load(root).unwrap();

        work::reset();
        let state = serde_json::to_value(build(&vault).unwrap()).unwrap();

        assert_eq!(work::count(Work::LinkSourceResolution), 0);
        let edges = state["graph"]["edges"].as_array().unwrap();
        for (kind, target) in [
            ("cites", "note:collision-note"),
            ("references", "code:src/helper.rs"),
            ("references", "pattern:ADR-0001/no-block-on"),
        ] {
            assert!(edges.iter().any(|edge| {
                edge["from"] == "note:ADR-0001" && edge["kind"] == kind && edge["to"] == target
            }));
        }
    }

    #[test]
    fn every_registered_policy_failure_blocks_state() {
        let root = policy_vault("criv-state-policy-failures");
        std::fs::write(
            root.join("docs/adr/0001-no-println.md"),
            POLICY_ADR.replace(
                "      pattern: \"println!($$$ARGS)\"",
                "      rule: \"not: [valid\"\n    - id: no-body\n      language: rust",
            ),
        )
        .unwrap();
        let vault = load(&root).unwrap();

        let error = build(&vault).unwrap_err().to_string();

        assert!(
            error.contains("policy pattern `no-body`")
                && error.contains("policy pattern `no-println`"),
            "{error}"
        );
    }

    #[test]
    fn disabled_source_index_writes_empty_source_state() {
        let root = unique_temp_dir("criv-disabled-source-state");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]

[index]
source = false
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();

        let vault = load(&root).unwrap();
        let state = build(&vault).unwrap();
        let json = serde_json::to_value(&state).unwrap();

        assert_eq!(json["source-index"].as_array().unwrap().len(), 0);
        assert!(
            json["graph"]["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|node| node["kind"] != "code")
        );
    }

    #[test]
    fn state_publishes_verified_documentation_assets() {
        let root = unique_temp_dir("criv-documentation-asset-state");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        let contents = b"\x89PNG\r\n\x1a\npreview";
        std::fs::write(root.join("docs/diagram.png"), contents).unwrap();

        let vault = load(&root).unwrap();
        let state = build(&vault).unwrap();
        let json = serde_json::to_value(&state).unwrap();

        assert_eq!(json["asset-index"][0]["path"], "docs/diagram.png");
        assert_eq!(json["asset-index"][0]["mime"], "image/png");
        assert_eq!(json["asset-index"][0]["bytes"], contents.len());
        assert_eq!(json["asset-index"][0]["hash"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn elixir_state_projects_kinds_relationships_labels_mime_and_rebuilds() {
        let root = unique_temp_dir("criv-elixir-state");
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(root.join("criv.toml"), "[source]\nroots = [\"lib\"]\n").unwrap();
        std::fs::write(root.join("lib/mix.exs"), "value = :ok\n").unwrap();
        let source = r#"
defmodule Demo.Behaviour do
  @callback run(term()) :: term()
  @macrocallback build(term()) :: term()
end

defprotocol Demo.Protocol do
  def work(value)
end

defmodule Demo.Target do
  def fetch(value), do: value
  def other(value), do: value
end

defmodule Demo.Record do
  defstruct [:value]
end

defmodule Demo.Error do
  defexception [:message]
end

defmodule Demo.App do
  alias Demo.Target, as: Target
  import Demo.Target, only: [fetch: 1]
  require Demo.Target
  use Demo.Target
  @behaviour Demo.Behaviour
  @behaviour External.Behaviour

  def run(value) do
    Target.fetch(value)
    External.fetch(value)
  end
  def capture(), do: &Target.fetch/1
  def dynamic(fun, value), do: fun.(value)
  defdelegate delegated(value), to: Target, as: :fetch
  defmacro build(value), do: value
  defguard valid(value) when is_integer(value)
end

defimpl Demo.Protocol, for: Demo.App do
  def work(value), do: value
end
"#;
        std::fs::write(root.join("lib/sample.ex"), source).unwrap();

        let vault = load(&root).unwrap();
        let first = build(&vault).unwrap();
        let decoded: StateDocument =
            serde_json::from_value(serde_json::to_value(&first).unwrap()).unwrap();
        assert_eq!(decoded.schema, STATE_SCHEMA);

        for kind in [
            "module",
            "behaviour",
            "protocol",
            "implementation",
            "struct",
            "exception",
            "function",
            "macro",
            "guard",
            "callback",
            "macro-callback",
            "alias",
            "import",
            "require",
            "use",
            "dynamic-call",
            "external-call",
            "external-module",
        ] {
            assert!(
                first
                    .wire
                    .graph
                    .nodes
                    .iter()
                    .any(|node| node.kind.as_str() == kind),
                "missing State node kind {kind}"
            );
        }
        for label in [
            "Demo.App",
            "Demo.Protocol for Demo.App",
            "run/1",
            "capture/0",
            "delegated/1",
        ] {
            assert!(
                first
                    .wire
                    .graph
                    .nodes
                    .iter()
                    .any(|node| node.label == label),
                "missing State label {label}"
            );
        }
        for kind in [
            "contains",
            "imports",
            "calls",
            "captures",
            "delegates",
            "implements-protocol",
            "implements-for",
            "implements-behaviour",
        ] {
            assert!(
                first
                    .wire
                    .graph
                    .edges
                    .iter()
                    .any(|edge| edge.kind.as_str() == kind),
                "missing State edge kind {kind}"
            );
        }
        let app = "symbol:lib/sample.ex#module:Demo.App";
        let run = "symbol:lib/sample.ex#module:Demo.App/fn:run/1";
        assert!(
            first.wire.graph.edges.iter().any(|edge| {
                edge.from == app && edge.to == run && edge.kind == EdgeKind::Contains
            })
        );
        assert!(
            first
                .wire
                .graph
                .nodes
                .iter()
                .all(|node| !node.hash.is_empty())
        );
        assert!(
            first
                .wire
                .graph
                .edges
                .iter()
                .all(|edge| !edge.hash.is_empty())
        );
        for path in ["lib/sample.ex", "lib/mix.exs"] {
            assert_eq!(
                first
                    .wire
                    .source_index
                    .iter()
                    .find(|entry| entry.path == path)
                    .and_then(|entry| entry.mime.as_deref()),
                Some("text/x-elixir")
            );
        }

        std::fs::write(
            root.join("lib/sample.ex"),
            source.replace("Target.fetch(value)", "Target.other(value)"),
        )
        .unwrap();
        let changed_vault = load(&root).unwrap();

        work::reset();
        let changed = build_incremental(&changed_vault, Some(&first)).unwrap();
        assert_eq!(work::count(Work::SourcePartitionBuild), 1);
        assert!(changed.wire.graph.edges.iter().any(|edge| {
            edge.from == run
                && edge.to == "symbol:lib/sample.ex#module:Demo.Target/fn:other/1"
                && edge.kind == EdgeKind::Calls
        }));
    }

    #[test]
    fn state_and_snapshot_writes_are_parseable() {
        let root = unique_temp_dir("criv-state-atomic-writes");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();

        let vault = load(&root).unwrap();
        work::reset();
        let (snapshot, _) = publish(&vault).unwrap();

        let state_path = root.join(".criv/state.json");
        let state: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(state_path).unwrap()).unwrap();
        assert_eq!(state["schema"], STATE_SCHEMA);

        let latest = std::fs::read_to_string(root.join(".criv/latest")).unwrap();
        assert_eq!(latest.trim(), snapshot);

        let snapshot_path = root
            .join(".criv/snapshots")
            .join(format!("{snapshot}.json"));
        let state_contents = std::fs::read_to_string(root.join(".criv/state.json")).unwrap();
        let snapshot_contents = std::fs::read_to_string(snapshot_path).unwrap();
        assert_eq!(state_contents, snapshot_contents);
        assert!(state_contents.ends_with('\n'));
        let snapshot_state: serde_json::Value = serde_json::from_str(&snapshot_contents).unwrap();
        assert_eq!(snapshot_state["schema"], STATE_SCHEMA);
        assert_eq!(
            work::counts([
                Work::SourcePartitionBuild,
                Work::NotePartitionBuild,
                Work::PolicyPartitionBuild,
                Work::StateSerialization,
            ]),
            [1, 0, 0, 1]
        );
    }

    #[test]
    fn corrupt_snapshot_preflight_keeps_the_prior_state_authoritative() {
        let root = unique_temp_dir("criv-state-publication-preflight");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("criv.toml"), "[source]\nroots = [\"src\"]\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn first() {}\n").unwrap();
        let first_vault = load(&root).unwrap();
        publish(&first_vault).unwrap();
        let prior_state = std::fs::read(root.join(".criv/state.json")).unwrap();

        let corrupt_hash = "a".repeat(64);
        std::fs::write(
            root.join(".criv/snapshots")
                .join(format!("{corrupt_hash}.json")),
            "{}\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn second() {}\n").unwrap();
        let second_vault = load(&root).unwrap();

        let error = publish(&second_vault).unwrap_err();
        assert!(error.to_string().contains("corrupt"));
        assert_eq!(
            std::fs::read(root.join(".criv/state.json")).unwrap(),
            prior_state
        );
        assert!(
            root.join(".criv/snapshots")
                .join(format!("{corrupt_hash}.json"))
                .exists()
        );
    }

    /// A deliberate compatibility check: a change to the fixture or the pinned
    /// hash changes the published State for every consumer (ADR-0146).
    #[test]
    fn serialized_state_matches_the_v1_contract_fixture() {
        let root = unique_temp_dir("criv-state-contract-fixture");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();
        std::fs::write(
            root.join("docs/adr/0001-entrypoint.md"),
            r#"---
id: ADR-0001
kind: decision
title: Entrypoint
status: accepted
governs:
  - src/lib.rs
policy:
  patterns:
    - id: entrypoint
      language: rust
      pattern: "fn $NAME() { $$$BODY }"
---

# Entrypoint
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("docs/adr/0002-draft-entrypoint.md"),
            r#"---
id: ADR-0002
kind: decision
title: Draft Entrypoint
status: draft
governs:
  - src/lib.rs
policy:
  patterns:
    - id: draft-entrypoint
      language: rust
      pattern: "fn $NAME() { $$$BODY }"
---

# Draft Entrypoint
"#,
        )
        .unwrap();

        let vault = load(&root).unwrap();
        let actual = serde_json::to_value(build(&vault).unwrap()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/state/criv.state.v1.json")).unwrap();

        assert_eq!(actual, expected);
        assert_eq!(
            actual["registered-patterns"],
            serde_json::json!(["ADR-0001/entrypoint"])
        );
        assert!(
            actual["patterns"]
                .get("ADR-0002/draft-entrypoint")
                .is_none()
        );
        assert_eq!(
            publish(&vault).unwrap().0,
            "5daebfeb1e0852bba748b8e225dab7469e53f98d3103e242ba0c3bffbbbb199a"
        );
    }

    #[cfg(unix)]
    #[test]
    fn state_writes_reject_a_symlinked_criv_directory() {
        use std::os::unix::fs::symlink;

        let root = unique_temp_dir("criv-state-symlink");
        let outside = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();
        symlink(outside.path(), root.join(".criv")).unwrap();

        let error = load(&root).unwrap_err();

        assert!(error.to_string().contains("symlinked vault path component"));
        assert!(!outside.path().join("state.json").exists());
    }

    const POLICY_ADR: &str = r#"---
id: ADR-0001
kind: decision
title: No Println
status: accepted
date: 2026-07-25
governs:
  - src/**/*.rs
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
      message: Prefer structured diagnostics.
---

# No Println

## Context

Context.

## Decision

Decision.

## Consequences

Consequences.
"#;

    const PATTERN_ID: &str = "ADR-0001/no-println";
    const DRAFT_PATTERN_ID: &str = "ADR-0002/no-debug";
    const FUNCTION_PATTERN_ID: &str = "ADR-0002/function";
    const FUNCTION_POLICY_ADR: &str = r#"---
id: ADR-0002
kind: decision
title: Functions Require Review
status: accepted
date: 2026-07-25
governs:
  - src/**/*.rs
policy:
  patterns:
    - id: function
      language: rust
      pattern: "fn $NAME() { $$$BODY }"
---

# Functions Require Review

## Context

Context.

## Decision

Decision.

## Consequences

Consequences.
"#;

    const DRAFT_POLICY_ADR: &str = r#"---
id: ADR-0002
kind: decision
title: No Debug
status: draft
date: 2026-08-02
governs:
  - src/**/*.rs
policy:
  patterns:
    - id: no-debug
      language: rust
      pattern: "println!($$$ARGS)"
---

# No Debug

## Context

Context.

## Decision

Decision.

## Consequences

Consequences.
"#;

    fn policy_vault(prefix: &str) -> TempRoot {
        let root = unique_temp_dir(prefix);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            "[vault]\ndocs = \"docs\"\nadr = \"adr\"\n\n[source]\nroots = [\"src\"]\n",
        )
        .unwrap();
        std::fs::write(root.join("docs/adr/0001-no-println.md"), POLICY_ADR).unwrap();
        std::fs::write(
            root.join("src/alpha.rs"),
            "fn alpha() {\n    println!(\"alpha\");\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/beta.rs"),
            "fn beta() {\n    println!(\"beta\");\n}\n",
        )
        .unwrap();
        root
    }

    fn matched_files(state: &State) -> Vec<String> {
        state
            .wire
            .patterns
            .get(PATTERN_ID)
            .map(|matches| matches.iter().map(|matched| matched.file.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn a_member_is_contained_by_its_class_beside_a_same_name_symbol() {
        let root = unique_temp_dir("criv-same-name-parent");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("criv.toml"), "[source]\nroots = [\"src\"]\n").unwrap();
        std::fs::write(
            root.join("src/thing.py"),
            "class Thing:\n    def run(self):\n        pass\n\n\ndef Thing():\n    pass\n",
        )
        .unwrap();

        let state = build(&load(&root).unwrap()).unwrap();

        assert!(state.wire.graph.edges.iter().any(|edge| {
            edge.from == "symbol:src/thing.py#type:Thing"
                && edge.to == "symbol:src/thing.py#type:Thing/member:run"
                && edge.kind == EdgeKind::Contains
        }));
    }

    #[test]
    fn no_op_incremental_build_rebuilds_no_partition() {
        let root = policy_vault("criv-state-partition-allocation-reuse");
        let first = build(&load(&root).unwrap()).unwrap();
        let vault = load(&root).unwrap();

        work::reset();
        let second = build_incremental(&vault, Some(&first)).unwrap();

        assert_eq!(
            work::counts([
                Work::SourcePartitionBuild,
                Work::NotePartitionBuild,
                Work::PolicyPartitionBuild,
            ]),
            [0, 0, 0]
        );
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&second).unwrap()
        );
    }

    #[test]
    fn state_batches_overlapping_adr_policies() {
        let root = policy_vault("criv-state-batched-policies");
        std::fs::write(
            root.join("docs/adr/0002-functions-require-review.md"),
            FUNCTION_POLICY_ADR,
        )
        .unwrap();
        let vault = load(&root).unwrap();
        work::reset();
        let state = build(&vault).unwrap();

        assert_eq!(
            work::count(Work::AstParse),
            2,
            "each eligible source file is parsed once for both overlapping ADR policies"
        );
        assert_eq!(matched_files(&state).len(), 2);
        assert_eq!(
            state.wire.patterns.get(FUNCTION_PATTERN_ID).unwrap().len(),
            2
        );
    }

    #[test]
    fn state_publishes_accepted_policy_patterns_only() {
        let root = policy_vault("criv-accepted-only-state");
        std::fs::write(root.join("docs/adr/0002-no-debug.md"), DRAFT_POLICY_ADR).unwrap();

        let vault = load(&root).unwrap();
        let state = build(&vault).unwrap();

        assert_eq!(state.wire.registered_patterns, vec![PATTERN_ID.to_string()]);
        assert!(state.wire.patterns.contains_key(PATTERN_ID));
        assert!(
            !state
                .wire
                .registered_patterns
                .contains(&DRAFT_PATTERN_ID.to_string())
        );
        assert!(!state.wire.patterns.contains_key(DRAFT_PATTERN_ID));
    }

    #[test]
    fn accepted_successor_removes_superseded_policy_state() {
        let root = policy_vault("criv-effective-policy-state");
        let vault = load(&root).unwrap();
        let (_, before) = publish(&vault).unwrap();
        assert!(before.wire.patterns.contains_key(PATTERN_ID));

        let successor = root.join("docs/adr/0002-successor.md");
        std::fs::write(
            &successor,
            r#"---
id: ADR-0002
kind: decision
title: Retire the old policy
status: accepted
supersedes:
  - ADR-0001
governs:
  - src/**
---

# Retire the old policy
"#,
        )
        .unwrap();
        let vault = load(&root).unwrap();
        let (_, after) = publish_incremental(&vault, Some(&before)).unwrap();

        assert!(
            !after
                .wire
                .registered_patterns
                .contains(&PATTERN_ID.to_string())
        );
        assert!(!after.wire.patterns.contains_key(PATTERN_ID));
    }

    #[test]
    fn incremental_policy_promotion_scans_every_governed_source() {
        let root = policy_vault("criv-policy-promotion");
        let policy_path = root.join("docs/adr/0002-no-debug.md");
        std::fs::write(&policy_path, DRAFT_POLICY_ADR).unwrap();

        let vault = load(&root).unwrap();
        let (_, before) = publish(&vault).unwrap();
        assert!(!before.wire.patterns.contains_key(DRAFT_PATTERN_ID));

        std::fs::write(
            &policy_path,
            DRAFT_POLICY_ADR.replace("status: draft", "status: accepted"),
        )
        .unwrap();
        let vault = load(&root).unwrap();
        let (_, after) = publish_incremental(&vault, Some(&before)).unwrap();

        assert_eq!(
            after.wire.patterns[DRAFT_PATTERN_ID]
                .iter()
                .map(|matched| matched.file.as_str())
                .collect::<Vec<_>>(),
            vec!["src/alpha.rs", "src/beta.rs"],
            "a newly accepted policy has no prior cache entry and must scan its whole scope"
        );
    }

    #[test]
    fn incremental_policy_demotion_removes_registration_and_cached_matches() {
        let root = policy_vault("criv-policy-demotion");
        let policy_path = root.join("docs/adr/0002-no-debug.md");
        let accepted = DRAFT_POLICY_ADR.replace("status: draft", "status: accepted");
        std::fs::write(&policy_path, &accepted).unwrap();

        let vault = load(&root).unwrap();
        let (_, before) = publish(&vault).unwrap();
        assert!(before.wire.patterns.contains_key(DRAFT_PATTERN_ID));

        std::fs::write(&policy_path, DRAFT_POLICY_ADR).unwrap();
        let vault = load(&root).unwrap();
        let (_, after) = publish_incremental(&vault, Some(&before)).unwrap();

        assert!(
            !after
                .wire
                .registered_patterns
                .contains(&DRAFT_PATTERN_ID.to_string())
        );
        assert!(!after.wire.patterns.contains_key(DRAFT_PATTERN_ID));
    }

    #[test]
    fn incremental_pattern_matches_reuse_unchanged_files() {
        let root = policy_vault("criv-incremental-pattern-reuse");

        let vault = load(&root).unwrap();
        let (_, first) = publish(&vault).unwrap();
        assert_eq!(
            matched_files(&first),
            vec!["src/alpha.rs".to_string(), "src/beta.rs".to_string()],
            "both governed files should match before the edit"
        );
        let alpha_before = first
            .wire
            .patterns
            .get(PATTERN_ID)
            .unwrap()
            .iter()
            .find(|matched| matched.file == "src/alpha.rs")
            .cloned()
            .expect("alpha match");

        std::fs::write(
            root.join("src/beta.rs"),
            "fn beta() {\n    // moved down\n    println!(\"beta changed\");\n}\n",
        )
        .unwrap();

        let vault = load(&root).unwrap();
        work::reset();
        let (_, second) = publish_incremental(&vault, Some(&first)).unwrap();

        assert_eq!(
            work::count(Work::AstParse),
            1,
            "only the changed governed file is rescanned"
        );
        assert_eq!(
            second.wire.registered_patterns,
            vec![PATTERN_ID.to_string()]
        );

        let second_matches = second.wire.patterns.get(PATTERN_ID).unwrap();
        let alpha_after = second_matches
            .iter()
            .find(|matched| matched.file == "src/alpha.rs")
            .expect("alpha match survives an unrelated edit");
        assert_eq!(
            &alpha_before, alpha_after,
            "an unchanged file's match must be carried forward byte-identically"
        );

        let beta_after = second_matches
            .iter()
            .find(|matched| matched.file == "src/beta.rs")
            .expect("beta match is rescanned");
        assert_ne!(
            beta_after.range, alpha_before.range,
            "the changed file's match should be rescanned at its new position"
        );
        assert_eq!(matched_files(&second).len(), 2);
    }

    #[test]
    fn incremental_pattern_matches_drop_deleted_files() {
        let root = policy_vault("criv-incremental-pattern-delete");

        let vault = load(&root).unwrap();
        let (_, first) = publish(&vault).unwrap();
        assert_eq!(matched_files(&first).len(), 2);

        std::fs::remove_file(root.join("src/beta.rs")).unwrap();

        let vault = load(&root).unwrap();
        let (_, second) = publish_incremental(&vault, Some(&first)).unwrap();

        assert_eq!(
            matched_files(&second),
            vec!["src/alpha.rs".to_string()],
            "a deleted file's match must not survive into the next state"
        );
    }

    struct TempRoot(tempfile::TempDir);

    impl std::ops::Deref for TempRoot {
        type Target = Path;

        fn deref(&self) -> &Path {
            self.0.path()
        }
    }

    fn unique_temp_dir(prefix: &str) -> TempRoot {
        TempRoot(tempfile::Builder::new().prefix(prefix).tempdir().unwrap())
    }
}
