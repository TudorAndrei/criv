use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

const STATE_SCHEMA: &str = "criv.state.v0";

#[wasm_bindgen]
pub fn validated_state(raw: &str) -> Result<JsValue, JsValue> {
    let state = decode_state_value(raw).map_err(|error| JsValue::from_str(&error))?;
    state
        .serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
        .map_err(|err| JsValue::from_str(&format!("failed to encode validated criv state: {err}")))
}

#[wasm_bindgen]
pub fn summarize_state(raw: &str) -> Result<JsValue, JsValue> {
    let state = parse_state(raw)?;
    let source_paths = unique_source_paths(&state.source_index);
    serde_wasm_bindgen::to_value(&StateSummary {
        schema: state.schema,
        node_count: state.graph.nodes.len(),
        edge_count: state.graph.edges.len(),
        source_count: source_paths.len(),
        pattern_count: state.registered_patterns.len(),
        first_node_id: state.graph.nodes.first().map(|node| node.id.clone()),
        first_edge: state
            .graph
            .edges
            .first()
            .map(|edge| format!("{}:{}:{}", edge.from, edge.kind, edge.to)),
        first_source_path: source_paths.into_iter().next(),
    })
    .map_err(|err| JsValue::from_str(&format!("failed to encode criv summary: {err}")))
}

#[wasm_bindgen]
pub fn source_entries(raw: &str) -> Result<JsValue, JsValue> {
    let state = parse_state(raw)?;
    let entries = unique_source_entries(&state.source_index);
    serde_wasm_bindgen::to_value(&entries)
        .map_err(|err| JsValue::from_str(&format!("failed to encode criv source entries: {err}")))
}

#[wasm_bindgen]
pub fn graph_nodes(raw: &str) -> Result<JsValue, JsValue> {
    let state = parse_state(raw)?;
    let nodes = editor_graph_nodes(&state);
    serde_wasm_bindgen::to_value(&nodes)
        .map_err(|err| JsValue::from_str(&format!("failed to encode criv graph nodes: {err}")))
}

#[wasm_bindgen]
pub fn suggest_source_selectors(raw: &str, query: &str, limit: usize) -> Result<JsValue, JsValue> {
    let state = parse_state(raw)?;
    let suggestions = source_selector_suggestions(&state, query, limit);
    serde_wasm_bindgen::to_value(&suggestions).map_err(|err| {
        JsValue::from_str(&format!(
            "failed to encode criv selector suggestions: {err}"
        ))
    })
}

#[wasm_bindgen]
pub fn lookup_graph_node(raw: &str, target: &str) -> Result<JsValue, JsValue> {
    let state = parse_state(raw)?;
    let node = find_editor_graph_node(&state, target);
    serde_wasm_bindgen::to_value(&node)
        .map_err(|err| JsValue::from_str(&format!("failed to encode criv graph node: {err}")))
}

fn parse_state(raw: &str) -> Result<CrivState, JsValue> {
    decode_state(raw).map_err(|error| JsValue::from_str(&error))
}

fn decode_state(raw: &str) -> Result<CrivState, String> {
    let state = serde_json::from_str::<CrivState>(raw)
        .map_err(|err| format!("invalid criv state JSON: {err}"))?;
    if state.schema != STATE_SCHEMA {
        return Err(format!("unsupported criv state schema: {}", state.schema));
    }
    Ok(state)
}

fn decode_state_value(raw: &str) -> Result<serde_json::Value, String> {
    let state = serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|err| format!("invalid criv state JSON: {err}"))?;
    let schema = state
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>");
    if schema != STATE_SCHEMA {
        return Err(format!("unsupported criv state schema: {schema}"));
    }
    Ok(state)
}

fn unique_source_paths(source_index: &[SourceIndexEntry]) -> Vec<String> {
    unique_source_entries(source_index)
        .into_iter()
        .map(|entry| entry.path)
        .collect()
}

fn unique_source_entries(source_index: &[SourceIndexEntry]) -> Vec<EditorSourceEntry> {
    let mut seen = BTreeSet::new();
    source_index
        .iter()
        .filter_map(|entry| {
            let path = safe_source_path(&entry.path)?;
            seen.insert(path.clone()).then_some(EditorSourceEntry {
                path,
                mime: entry.mime.clone(),
                frecency: entry.frecency,
            })
        })
        .collect()
}

fn safe_source_path(path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with("//")
        || path.contains('\0')
        || path.as_bytes().get(1) == Some(&b':')
    {
        return None;
    }
    let mut normalized = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            segment => normalized.push(segment),
        }
    }
    (!normalized.is_empty()).then(|| normalized.join("/"))
}

fn editor_graph_nodes(state: &CrivState) -> Vec<EditorGraphNode> {
    state
        .graph
        .nodes
        .iter()
        .map(|node| EditorGraphNode {
            id: node.id.clone(),
            kind: node.kind.clone().unwrap_or_default(),
            label: node.label.clone().unwrap_or_else(|| node.id.clone()),
            path: node.path.clone(),
            source_target: source_target(node),
            line_range: node.path.as_deref().and_then(line_range),
        })
        .collect()
}

fn source_selector_suggestions(
    state: &CrivState,
    query: &str,
    limit: usize,
) -> Vec<SourceSelectorSuggestion> {
    let clean_query = query.trim().to_lowercase();
    let mut seen = BTreeSet::new();
    let mut suggestions = Vec::new();

    for entry in unique_source_entries(&state.source_index) {
        if !seen.insert(entry.path.clone()) {
            continue;
        }
        let score = if clean_query.is_empty() {
            0
        } else if let Some(score) = source_match_score(&entry.path, &clean_query) {
            score + i64::from(entry.frecency)
        } else {
            continue;
        };
        suggestions.push(ScoredSourceSelectorSuggestion {
            suggestion: SourceSelectorSuggestion {
                target: entry.path.clone(),
                label: entry.path.clone(),
                kind: "file".into(),
                path: entry.path,
                detail: "file".into(),
            },
            score,
            frecency: entry.frecency,
        });
    }

    for node in editor_graph_nodes(state) {
        let Some(target) = node.source_target.clone() else {
            continue;
        };
        if !target.contains('#') || !seen.insert(target.clone()) {
            continue;
        }
        let score = if clean_query.is_empty() {
            0
        } else if let Some(score) = source_match_score(&target, &clean_query) {
            score
        } else {
            continue;
        };
        suggestions.push(ScoredSourceSelectorSuggestion {
            suggestion: SourceSelectorSuggestion {
                target,
                label: node.label,
                kind: node.kind,
                path: node.path.unwrap_or_default(),
                detail: node.id,
            },
            score,
            frecency: 0,
        });
    }

    suggestions.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.frecency.cmp(&left.frecency))
            .then_with(|| left.suggestion.target.cmp(&right.suggestion.target))
    });
    suggestions.truncate(limit);
    suggestions
        .into_iter()
        .map(|scored| scored.suggestion)
        .collect()
}

fn find_editor_graph_node(state: &CrivState, target: &str) -> Option<EditorGraphNode> {
    editor_graph_nodes(state).into_iter().find(|node| {
        node.id == target
            || node.source_target.as_deref() == Some(target)
            || node.path.as_deref() == Some(target)
    })
}

fn source_target(node: &Node) -> Option<String> {
    node.id
        .strip_prefix("symbol:")
        .or_else(|| node.id.strip_prefix("code:"))
        .map(ToString::to_string)
}

fn line_range(path: &str) -> Option<String> {
    path.split_once("#L").map(|(_, range)| format!("L{range}"))
}

fn source_match_score(path: &str, query: &str) -> Option<i64> {
    let lower_path = path.to_lowercase();
    let basename = lower_path.rsplit('/').next().unwrap_or(&lower_path);
    if lower_path == query {
        return Some(100_000);
    }
    if basename == query {
        return Some(90_000);
    }
    if lower_path.ends_with(query) {
        return Some(80_000 - lower_path.len() as i64);
    }
    if basename.starts_with(query) {
        return Some(70_000 - basename.len() as i64);
    }
    if let Some(index) = lower_path.find(query) {
        return Some(60_000 - index as i64 - lower_path.len() as i64);
    }
    fuzzy_subsequence_score(&lower_path, query)
        .map(|score| 40_000 + score - lower_path.len() as i64)
}

fn fuzzy_subsequence_score(value: &str, query: &str) -> Option<i64> {
    let mut query_chars = query.chars();
    let mut current_query = query_chars.next();
    let mut score = 0;
    let mut run = 0;
    let mut previous = None;

    for character in value.chars() {
        let Some(query_character) = current_query else {
            break;
        };
        if character != query_character {
            run = 0;
            previous = Some(character);
            continue;
        }
        run += 1;
        let boundary_bonus = if previous.is_none() || previous == Some('/') {
            8
        } else {
            0
        };
        score += run * 3 + boundary_bonus;
        current_query = query_chars.next();
        previous = Some(character);
    }

    current_query.is_none().then_some(score)
}

#[derive(Debug, Deserialize)]
struct CrivState {
    schema: String,
    #[serde(default)]
    graph: Graph,
    #[serde(default, rename = "registered-patterns")]
    registered_patterns: Vec<String>,
    #[serde(default, rename = "source-index")]
    source_index: Vec<SourceIndexEntry>,
}

#[derive(Debug, Default, Deserialize)]
struct Graph {
    #[serde(default)]
    nodes: Vec<Node>,
    #[serde(default)]
    edges: Vec<Edge>,
}

#[derive(Debug, Deserialize)]
struct Node {
    id: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Edge {
    from: String,
    to: String,
    kind: String,
}

#[derive(Debug, Deserialize)]
struct SourceIndexEntry {
    path: String,
    #[serde(default)]
    mime: Option<String>,
    #[serde(default)]
    frecency: u32,
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

#[derive(Debug, Serialize)]
struct EditorSourceEntry {
    path: String,
    mime: Option<String>,
    frecency: u32,
}

#[derive(Debug, Clone, Serialize)]
struct EditorGraphNode {
    id: String,
    kind: String,
    label: String,
    path: Option<String>,
    source_target: Option<String>,
    line_range: Option<String>,
}

#[derive(Debug, Serialize)]
struct SourceSelectorSuggestion {
    target: String,
    label: String,
    kind: String,
    path: String,
    detail: String,
}

struct ScoredSourceSelectorSuggestion {
    suggestion: SourceSelectorSuggestion,
    score: i64,
    frecency: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shared_state_contract_fixture() {
        let state =
            parse_state(include_str!("../../../fixtures/state/criv.state.v0.json")).unwrap();
        assert_eq!(state.schema, "criv.state.v0");
        assert_eq!(state.graph.nodes.len(), 6);
        assert_eq!(state.graph.edges.len(), 5);
        assert_eq!(state.registered_patterns, ["ADR-0001/entrypoint"]);
        assert_eq!(state.registered_patterns.len(), 1);
        assert_eq!(state.source_index.len(), 1);
        assert_eq!(state.source_index[0].frecency, 0);
    }

    #[test]
    fn rejects_a_wrong_state_schema() {
        let raw = include_str!("../../../fixtures/state/criv.state.v0.json")
            .replace("criv.state.v0", "criv.state.v1");

        assert!(decode_state(&raw).is_err());
        assert!(decode_state_value(&raw).is_err());
    }

    #[test]
    fn validated_state_preserves_host_consumed_fields() {
        let state =
            decode_state_value(include_str!("../../../fixtures/state/criv.state.v0.json")).unwrap();

        assert_eq!(state["schema"], "criv.state.v0");
        assert_eq!(state["registered-patterns"][0], "ADR-0001/entrypoint");
        assert!(state["patterns"]["ADR-0001/entrypoint"].is_array());
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

        assert_eq!(by_id.id, by_target.id);
    }

    fn editor_state() -> CrivState {
        serde_json::from_str(
            r#"{
              "schema": "criv.state.v0",
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

    fn selector_state() -> CrivState {
        serde_json::from_str(
            r#"{
              "schema": "criv.state.v0",
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
