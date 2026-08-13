use std::collections::{BTreeMap, HashMap};

#[cfg(target_arch = "wasm32")]
use criv_state_wire::is_supported_schema;
#[cfg(test)]
use criv_state_wire::{Node, SourceIndexEntry};
use criv_state_wire::{PatternMatch, StateDocument};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

mod decode;
mod likec4;
mod projection;
mod source;

const INITIAL_PROJECTIONS_TAKEN: &str = "criv initial projections were already taken";

#[wasm_bindgen]
pub struct LoadedState {
    #[cfg(not(target_arch = "wasm32"))]
    initial_envelope: Option<serde_json::Value>,
    initial_projections: Option<JsValue>,
    prepared: PreparedState,
}

#[wasm_bindgen]
impl LoadedState {
    #[wasm_bindgen(constructor)]
    pub fn new(raw: &str) -> Result<LoadedState, JsValue> {
        #[cfg(target_arch = "wasm32")]
        {
            return Self::load_wasm(raw);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut loaded = Self::load(raw).map_err(|error| JsValue::from_str(&error))?;
            loaded.prepare_initial_projections()?;
            Ok(loaded)
        }
    }

    #[wasm_bindgen(js_name = initialProjections)]
    pub fn initial_projections(&mut self) -> Result<JsValue, JsValue> {
        self.initial_projections
            .take()
            .ok_or_else(|| JsValue::from_str(INITIAL_PROJECTIONS_TAKEN))
    }

    #[wasm_bindgen(js_name = lookupSourceTarget)]
    pub fn lookup_source_target(&self, target: &str) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.prepared.lookup_source_target(target)).map_err(|error| {
            JsValue::from_str(&format!(
                "failed to encode criv source-target lookup result: {error}"
            ))
        })
    }

    #[wasm_bindgen(js_name = suggestSelectors)]
    pub fn suggest_selectors(&self, query: &str, limit: usize) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.prepared.suggest_selectors(query, limit)).map_err(
            |error| {
                JsValue::from_str(&format!(
                    "failed to encode criv selector suggestions: {error}"
                ))
            },
        )
    }
}

impl LoadedState {
    #[cfg(target_arch = "wasm32")]
    fn load_wasm(raw: &str) -> Result<Self, JsValue> {
        let envelope = js_sys::JSON::parse(raw).map_err(|error| {
            JsValue::from_str(&format!(
                "criv-state-json-invalid: invalid criv state JSON: {}",
                decode::js_error_message(&error)
            ))
        })?;
        let schema = js_sys::Reflect::get(&envelope, &JsValue::from_str("schema"))
            .ok()
            .and_then(|value| value.as_string())
            .unwrap_or_else(|| "<missing>".into());
        if !is_supported_schema(&schema) {
            return Err(JsValue::from_str(&format!(
                "criv-state-schema-unsupported: unsupported criv state schema: {schema}"
            )));
        }
        decode::validate_architecture_wrapper_wasm(&envelope)?;
        let state = serde_wasm_bindgen::from_value::<StateDocument>(envelope)
            .map_err(|error| JsValue::from_str(&format!("criv-state-json-invalid: {error}")))?;
        let prepared = PreparedState::new(state).map_err(|error| JsValue::from_str(&error))?;
        let initial_projections = projection::initial_projections_from_js(&prepared)?;
        Ok(Self {
            #[cfg(not(target_arch = "wasm32"))]
            initial_envelope: None,
            initial_projections: Some(initial_projections),
            prepared,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn prepare_initial_projections(&mut self) -> Result<(), JsValue> {
        let projections = self
            .initial_envelope
            .as_ref()
            .ok_or_else(|| JsValue::from_str(INITIAL_PROJECTIONS_TAKEN))?;
        let value = projections
            .serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
            .map_err(|error| {
                JsValue::from_str(&format!(
                    "failed to encode criv initial projections: {error}"
                ))
            })?;
        self.initial_envelope = None;
        self.initial_projections = Some(value);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load(raw: &str) -> Result<Self, String> {
        let envelope = decode::decode_state_value(raw)?;
        decode::validate_architecture_wrapper(&envelope)?;
        let state = StateDocument::deserialize(&envelope)
            .map_err(|error| format!("criv-state-json-invalid: {error}"))?;
        let prepared = PreparedState::new(state)?;
        let initial_envelope =
            serde_json::to_value(projection::InitialProjections::from(&prepared))
                .map_err(|error| format!("failed to encode criv initial projections: {error}"))?;
        Ok(Self {
            initial_envelope: Some(initial_envelope),
            initial_projections: None,
            prepared,
        })
    }

    #[cfg(test)]
    fn take_initial_projection(&mut self) -> Result<serde_json::Value, String> {
        self.initial_envelope
            .take()
            .ok_or_else(|| INITIAL_PROJECTIONS_TAKEN.to_string())
    }
}

impl PreparedState {
    fn new(state: StateDocument) -> Result<Self, String> {
        let StateDocument {
            schema,
            architecture,
            graph,
            mut registered_patterns,
            patterns,
            source_index,
        } = state;
        let criv_state_wire::Graph { nodes, edges, .. } = graph;
        let sources = source::take_unique_source_entries(source_index);
        let nodes = source::take_editor_graph_nodes(nodes);
        registered_patterns.sort();
        registered_patterns.dedup();
        let architecture = likec4::prepare_architecture(architecture)?;
        let c4_artifacts = likec4::prepare_c4_artifacts(&sources, &nodes);
        let summary = StateSummary {
            schema,
            node_count: nodes.len(),
            edge_count: edges.len(),
            source_count: sources.len(),
            pattern_count: registered_patterns.len(),
            first_node_id: nodes.first().map(|node| node.id.clone()),
            first_edge: edges
                .first()
                .map(|edge| format!("{}:{}:{}", edge.from, edge.kind, edge.to)),
            first_source_path: sources.first().map(|source| source.path.clone()),
        };
        Ok(Self::from_parts(
            summary,
            sources,
            nodes,
            registered_patterns,
            patterns,
            architecture,
            c4_artifacts,
        ))
    }

    #[cfg(test)]
    fn from_borrowed(state: &StateDocument) -> Self {
        Self::new(state.clone()).expect("test State must prepare")
    }
}

#[derive(Debug, Serialize)]
struct StateSummary {
    schema: String,
    node_count: usize,
    edge_count: usize,
    source_count: usize,
    pattern_count: usize,
    first_node_id: Option<String>,
    first_edge: Option<String>,
    first_source_path: Option<String>,
}

struct PreparedState {
    summary: StateSummary,
    sources: Vec<EditorSourceEntry>,
    nodes: Vec<EditorGraphNode>,
    registered_patterns: Vec<String>,
    pattern_matches: BTreeMap<String, Vec<PatternMatch>>,
    architecture: Option<EditorLikeC4Model>,
    c4_artifacts: Vec<EditorC4Artifact>,
    exact_source_lookup: HashMap<u64, Vec<usize>>,
    legacy_source_lookup: HashMap<u64, Vec<usize>>,
    selectors: Vec<PreparedSelector>,
    empty_selector_order: Vec<usize>,
}

#[derive(Debug, Serialize)]
struct EditorSourceEntry {
    path: String,
    mime: Option<String>,
    frecency: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct EditorGraphNode {
    id: String,
    kind: String,
    label: String,
    path: Option<String>,
    source_target: Option<String>,
    line_range: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct EditorLikeC4View {
    id: String,
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct EditorLikeC4SourceLink {
    element: String,
    target: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EditorLikeC4Model {
    protocol_version: u32,
    likec4_version: String,
    workspace: String,
    model: serde_json::Value,
    views: Vec<EditorLikeC4View>,
    source_links: Vec<EditorLikeC4SourceLink>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishedLikeC4Model {
    raw: serde_json::Value,
    views: Vec<EditorLikeC4View>,
    source_links: Vec<EditorLikeC4SourceLink>,
}

#[derive(Debug, Clone, Serialize)]
struct EditorC4Artifact {
    path: String,
    label: String,
    target: String,
}

const MAX_AMBIGUOUS_SOURCE_CANDIDATES: usize = 5;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum SourceTargetLookupResult {
    Resolved {
        canonical_target: String,
        node: EditorGraphNode,
    },
    Unresolved,
    Ambiguous {
        candidates: Vec<SourceTargetCandidate>,
        total_candidate_count: usize,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct SourceTargetCandidate {
    canonical_target: String,
    node_id: String,
    kind: String,
    label: String,
}

#[derive(Debug, Clone, Serialize)]
struct SourceSelectorSuggestion {
    target: String,
    label: String,
    kind: String,
    path: String,
    detail: String,
}

struct PreparedSelector {
    entry: SelectorEntry,
    frecency: u32,
}

#[derive(Clone, Copy)]
enum SelectorEntry {
    Source(usize),
    Node(usize),
}

#[cfg(test)]
mod tests {
    use super::decode::decode_state_value;
    use super::source::{
        editor_graph_nodes, find_editor_graph_node, source_selector_suggestions,
        unique_source_entries, unique_source_paths,
    };
    use super::*;

    #[test]
    fn prepares_the_complete_editor_projection_from_the_shared_fixture() {
        let fixture = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../fixtures/editor/likec4-projection.v1.json"
        ))
        .unwrap();
        let loaded = LoadedState::load(&fixture["state"].to_string()).unwrap();

        assert_eq!(
            serde_json::to_value(&loaded.prepared.registered_patterns).unwrap(),
            fixture["expected"]["registeredPatterns"]
        );
        assert_eq!(
            serde_json::to_value(&loaded.prepared.pattern_matches).unwrap(),
            fixture["state"]["patterns"]
        );
        assert_eq!(
            serde_json::to_value(&loaded.prepared.c4_artifacts).unwrap(),
            fixture["expected"]["c4Artifacts"]
        );
        assert_eq!(
            serde_json::to_value(&loaded.prepared.architecture).unwrap(),
            fixture["expected"]["architecture"]
        );
    }

    #[test]
    fn rejects_each_invalid_architecture_contract() {
        let fixture = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../fixtures/editor/likec4-projection.v1.json"
        ))
        .unwrap();

        let mut wrong_protocol = fixture["state"].clone();
        wrong_protocol["architecture"]["protocolVersion"] = 2.into();
        assert!(
            load_error(&wrong_protocol.to_string())
                .starts_with("criv-likec4-protocol-unsupported:")
        );

        let mut wrong_version = fixture["state"].clone();
        wrong_version["architecture"]["likec4Version"] = "2.0.0".into();
        assert!(
            load_error(&wrong_version.to_string()).starts_with("criv-likec4-version-unsupported:")
        );

        let mut invalid_model = fixture["state"].clone();
        invalid_model["architecture"]["model"]["raw"] = true.into();
        assert!(load_error(&invalid_model.to_string()).starts_with("criv-likec4-model-invalid:"));
    }

    fn load_error(raw: &str) -> String {
        match LoadedState::load(raw) {
            Ok(_) => panic!("expected State load to fail"),
            Err(error) => error,
        }
    }

    #[test]
    fn loaded_state_prepares_one_revision_for_all_operations() {
        let mut loaded =
            LoadedState::load(include_str!("../../../fixtures/state/criv.state.v1.json")).unwrap();

        assert_eq!(loaded.prepared.summary.node_count, 6);
        assert_eq!(loaded.prepared.sources.len(), 1);
        assert_eq!(loaded.prepared.nodes.len(), 6);
        let SourceTargetLookupResult::Resolved { node, .. } =
            loaded.prepared.lookup_source_target("src/lib.rs#fn:run")
        else {
            panic!("expected an exact source target to resolve");
        };
        assert_eq!(node.kind, "function");
        assert_eq!(
            loaded.prepared.suggest_selectors("run", 10)[0].target,
            "src/lib.rs#fn:run"
        );

        let projections = loaded.take_initial_projection().unwrap();
        assert!(projections.get("state").is_none());
        assert_eq!(projections["summary"]["schema"], "criv.state.v1");
        assert_eq!(projections["registeredPatterns"][0], "ADR-0001/entrypoint");
        assert!(projections["patternMatches"]["ADR-0001/entrypoint"].is_array());
        assert!(loaded.take_initial_projection().is_err());
    }

    #[test]
    fn parses_shared_state_contract_fixture() {
        let state = serde_json::from_str::<StateDocument>(include_str!(
            "../../../fixtures/state/criv.state.v1.json"
        ))
        .unwrap();
        assert_eq!(state.schema, "criv.state.v1");
        assert_eq!(state.graph.nodes.len(), 6);
        assert_eq!(state.graph.edges.len(), 5);
        assert_eq!(state.registered_patterns, ["ADR-0001/entrypoint"]);
        assert_eq!(state.registered_patterns.len(), 1);
        assert_eq!(state.source_index.len(), 1);
        assert_eq!(state.source_index[0].frecency, 0);
    }

    #[test]
    fn rejects_a_wrong_state_schema() {
        let raw = include_str!("../../../fixtures/state/criv.state.v1.json")
            .replace("criv.state.v1", "criv.state.v2");

        assert!(LoadedState::load(&raw).is_err());
        assert!(decode_state_value(&raw).is_err());
    }

    #[test]
    fn initial_projection_does_not_publish_the_raw_state() {
        let mut loaded =
            LoadedState::load(include_str!("../../../fixtures/state/criv.state.v1.json")).unwrap();
        let projections = loaded.take_initial_projection().unwrap();

        assert!(projections.get("state").is_none());
        assert!(projections.get("graph").is_none());
        assert!(projections.get("source-index").is_none());
    }

    #[test]
    fn deduplicates_source_paths_for_summary() {
        let source_index = vec![
            SourceIndexEntry {
                path: "src/lib.rs".into(),
                mime: None,
                frecency: 0,
            },
            SourceIndexEntry {
                path: "src/lib.rs".into(),
                mime: None,
                frecency: 0,
            },
            SourceIndexEntry {
                path: "src/main.rs".into(),
                mime: None,
                frecency: 0,
            },
        ];

        let paths = unique_source_paths(&source_index);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths, vec!["src/lib.rs", "src/main.rs"]);
    }

    #[test]
    fn source_entries_reject_escaping_paths_and_normalize_separators() {
        let entries = unique_source_entries(&[
            SourceIndexEntry {
                path: "src/lib.rs".into(),
                mime: None,
                frecency: 1,
            },
            SourceIndexEntry {
                path: "../secret".into(),
                mime: None,
                frecency: 2,
            },
            SourceIndexEntry {
                path: "/etc/passwd".into(),
                mime: None,
                frecency: 3,
            },
            SourceIndexEntry {
                path: "C:\\secret".into(),
                mime: None,
                frecency: 4,
            },
            SourceIndexEntry {
                path: "src\\windows\\path.rs".into(),
                mime: None,
                frecency: 5,
            },
        ]);

        assert_eq!(
            entries
                .into_iter()
                .map(|entry| entry.path)
                .collect::<Vec<_>>(),
            ["src/lib.rs", "src/windows/path.rs"]
        );
    }

    #[test]
    fn graph_nodes_include_source_targets_and_line_ranges() {
        let state = editor_state();

        let nodes = editor_graph_nodes(&state);
        let symbol = nodes
            .iter()
            .find(|node| node.id == "symbol:src/lib.rs#fn:run")
            .unwrap();

        assert_eq!(symbol.kind, "function");
        assert_eq!(symbol.label, "run");
        assert_eq!(symbol.source_target.as_deref(), Some("src/lib.rs#fn:run"));
        assert_eq!(symbol.line_range.as_deref(), Some("L10-L20"));
    }

    #[test]
    fn selector_suggestions_include_files_and_symbols() {
        let state = editor_state();

        let suggestions = source_selector_suggestions(&state, "run", 10);

        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].target, "src/lib.rs#fn:run");
        assert_eq!(suggestions[0].kind, "function");
        assert_eq!(suggestions[1].target, "src/run.rs");
        assert_eq!(suggestions[1].kind, "file");
    }

    #[test]
    fn selector_suggestions_rank_with_weighted_source_scoring() {
        let state = selector_state();

        let suggestions = source_selector_suggestions(&state, "lib.rs", 10);
        let targets = suggestion_targets(&suggestions);

        assert_eq!(
            targets,
            vec![
                "lib.rs",
                "crates/criv-wasm/src/lib.rs",
                "src/lib.rs",
                "src/slow_lib.rs",
                "src/lib.rs#fn:run"
            ]
        );
    }

    #[test]
    fn selector_suggestions_use_frecency_as_tiebreaker() {
        let state = selector_state();

        let suggestions = source_selector_suggestions(&state, "src/tie", 10);
        let targets = suggestion_targets(&suggestions);

        assert_eq!(targets, vec!["src/tie-high.rs", "src/tie-low.rs"]);
    }

    #[test]
    fn selector_suggestions_drop_non_matches_and_keep_fuzzy_matches() {
        let state = selector_state();

        let suggestions = source_selector_suggestions(&state, "slr", 10);
        let targets = suggestion_targets(&suggestions);

        assert!(targets.contains(&"src/lib.rs"));
        assert!(targets.contains(&"src/lib.rs#fn:run"));
        assert!(!targets.contains(&"docs/adr.md"));
    }

    #[test]
    fn selector_suggestions_empty_query_orders_by_frecency_and_target() {
        let state = selector_state();

        let suggestions = source_selector_suggestions(&state, "", 4);
        let targets = suggestion_targets(&suggestions);

        assert_eq!(
            targets,
            vec![
                "src/tie-high.rs",
                "crates/criv-wasm/src/lib.rs",
                "src/lib.rs",
                "src/main.rs"
            ]
        );
    }

    #[test]
    fn selector_suggestions_respect_limit() {
        let state = selector_state();

        let suggestions = source_selector_suggestions(&state, "", 2);

        assert_eq!(suggestions.len(), 2);
    }

    #[test]
    fn lookup_finds_graph_nodes_by_id_or_source_target() {
        let state = editor_state();

        let by_id = find_editor_graph_node(&state, "symbol:src/lib.rs#fn:run").unwrap();
        let by_target = find_editor_graph_node(&state, "src/lib.rs#fn:run").unwrap();
        let by_legacy_symbol = find_editor_graph_node(&state, "src/lib.rs#run").unwrap();
        let by_basename = find_editor_graph_node(&state, "lib.rs").unwrap();

        assert_eq!(by_id.id, by_target.id);
        assert_eq!(by_id.id, by_legacy_symbol.id);
        assert_eq!(by_basename.id, "code:src/lib.rs");
    }

    #[test]
    fn lookup_rejects_ambiguous_legacy_aliases() {
        let mut state = editor_state();
        state.graph.nodes.push(Node {
            id: "symbol:src/lib.rs#method:run".into(),
            hash: String::new(),
            kind: "method".into(),
            label: "run".into(),
            path: Some("src/lib.rs#L30-L40".into()),
        });

        assert!(find_editor_graph_node(&state, "src/lib.rs#run").is_none());
        assert!(find_editor_graph_node(&state, "src/lib.rs#fn:run").is_some());
    }

    #[test]
    fn source_target_lookup_matches_the_shared_editor_fixture() {
        let fixture = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../fixtures/editor/source-target-lookup.v1.json"
        ))
        .unwrap();
        let state = serde_json::from_value::<StateDocument>(fixture["state"].clone()).unwrap();
        let prepared = PreparedState::from_borrowed(&state);

        for case in fixture["cases"].as_array().unwrap() {
            let target = case["target"].as_str().unwrap();
            let actual = serde_json::to_value(prepared.lookup_source_target(target)).unwrap();
            assert_eq!(actual["kind"], case["kind"], "lookup kind for {target}");
            if let Some(expected) = case.get("canonical_target") {
                assert_eq!(
                    actual["canonical_target"], *expected,
                    "canonical target for {target}"
                );
            }
            if let Some(expected) = case.get("total_candidate_count") {
                assert_eq!(
                    actual["total_candidate_count"], *expected,
                    "candidate count for {target}"
                );
            }
            if target == "common.rs" {
                let candidates = actual["candidates"].as_array().unwrap();
                assert_eq!(candidates.len(), MAX_AMBIGUOUS_SOURCE_CANDIDATES);
                assert_eq!(candidates[0]["canonical_target"], "a/common.rs");
                assert_eq!(candidates[4]["canonical_target"], "e/common.rs");
            }
        }

        let mut reversed =
            serde_json::from_value::<StateDocument>(fixture["state"].clone()).unwrap();
        reversed.graph.nodes.reverse();
        let reversed = PreparedState::from_borrowed(&reversed);
        for target in ["src/lib.rs#run", "src/dup.rs#fn:work", "common.rs"] {
            assert_eq!(
                serde_json::to_value(prepared.lookup_source_target(target)).unwrap(),
                serde_json::to_value(reversed.lookup_source_target(target)).unwrap(),
                "lookup result must not depend on State order for {target}"
            );
        }
    }

    fn editor_state() -> StateDocument {
        serde_json::from_str(
            r#"{
              "schema": "criv.state.v1",
              "graph": {
                "nodes": [
                  {
                    "id": "code:src/lib.rs",
                    "kind": "code",
                    "label": "src/lib.rs (rust)",
                    "path": "src/lib.rs"
                  },
                  {
                    "id": "symbol:src/lib.rs#fn:run",
                    "kind": "function",
                    "label": "run",
                    "path": "src/lib.rs#L10-L20"
                  }
                ],
                "edges": []
              },
              "registered-patterns": [],
              "source-index": [
                { "path": "src/lib.rs", "frecency": 4, "mime": "text/rust" },
                { "path": "src/lib.rs", "frecency": 1 },
                { "path": "src/run.rs", "frecency": 3 }
              ]
            }"#,
        )
        .unwrap()
    }

    fn selector_state() -> StateDocument {
        serde_json::from_str(
            r#"{
              "schema": "criv.state.v1",
              "graph": {
                "nodes": [
                  {
                    "id": "symbol:src/lib.rs#fn:run",
                    "kind": "function",
                    "label": "run",
                    "path": "src/lib.rs#L10-L20"
                  },
                  {
                    "id": "symbol:src/main.rs#fn:start",
                    "kind": "function",
                    "label": "start",
                    "path": "src/main.rs#L4-L8"
                  }
                ],
                "edges": []
              },
              "registered-patterns": [],
              "source-index": [
                { "path": "src/tie-low.rs", "frecency": 1 },
                { "path": "src/tie-high.rs", "frecency": 50 },
                { "path": "crates/criv-wasm/src/lib.rs", "frecency": 40 },
                { "path": "src/lib.rs", "frecency": 5 },
                { "path": "lib.rs", "frecency": 0 },
                { "path": "src/slow_lib.rs", "frecency": 0 },
                { "path": "src/main.rs", "frecency": 2 },
                { "path": "docs/adr.md", "frecency": 0 }
              ]
            }"#,
        )
        .unwrap()
    }

    fn suggestion_targets(suggestions: &[SourceSelectorSuggestion]) -> Vec<&str> {
        suggestions
            .iter()
            .map(|suggestion| suggestion.target.as_str())
            .collect()
    }
}
