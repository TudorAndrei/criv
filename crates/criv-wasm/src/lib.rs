use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

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
    serde_json::from_str::<CrivState>(raw)
        .map_err(|err| JsValue::from_str(&format!("invalid criv state JSON: {err}")))
}

fn unique_source_paths(source_index: &[SourceIndexEntry]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    source_index
        .iter()
        .filter(|entry| !entry.path.is_empty() && seen.insert(entry.path.clone()))
        .map(|entry| entry.path.clone())
        .collect()
}

fn unique_source_entries(source_index: &[SourceIndexEntry]) -> Vec<EditorSourceEntry> {
    let mut seen = BTreeSet::new();
    source_index
        .iter()
        .filter(|entry| !entry.path.is_empty() && seen.insert(entry.path.clone()))
        .map(|entry| EditorSourceEntry {
            path: entry.path.clone(),
            mime: entry.mime.clone(),
            frecency: entry.frecency,
        })
        .collect()
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
        if !matches_query(&entry.path, &clean_query) || !seen.insert(entry.path.clone()) {
            continue;
        }
        suggestions.push(SourceSelectorSuggestion {
            target: entry.path.clone(),
            label: entry.path.clone(),
            kind: "file".into(),
            path: entry.path,
            detail: "file".into(),
        });
    }

    for node in editor_graph_nodes(state) {
        let Some(target) = node.source_target.clone() else {
            continue;
        };
        if !target.contains('#')
            || !matches_query(&target, &clean_query)
            || !seen.insert(target.clone())
        {
            continue;
        }
        suggestions.push(SourceSelectorSuggestion {
            target,
            label: node.label,
            kind: node.kind,
            path: node.path.unwrap_or_default(),
            detail: node.id,
        });
    }

    suggestions.sort_by(|left, right| {
        selector_rank(&left.target, &clean_query)
            .cmp(&selector_rank(&right.target, &clean_query))
            .then_with(|| left.target.cmp(&right.target))
    });
    suggestions.truncate(limit);
    suggestions
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

fn matches_query(candidate: &str, query: &str) -> bool {
    query.is_empty() || candidate.to_lowercase().contains(query)
}

fn selector_rank(candidate: &str, query: &str) -> usize {
    if query.is_empty() {
        return 0;
    }
    let candidate = candidate.to_lowercase();
    if candidate == query {
        0
    } else if candidate.starts_with(query) {
        1
    } else {
        2
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_state_shape() {
        let raw = r#"{
          "schema": "criv.state.v0",
          "graph": { "nodes": [{ "id": "note:x" }], "edges": [] },
          "registered-patterns": ["legacy"],
          "source-index": [{ "path": "src/lib.rs", "frecency": 7, "mime": "text/rust" }]
        }"#;

        let state = serde_json::from_str::<CrivState>(raw).unwrap();
        assert_eq!(state.schema, "criv.state.v0");
        assert_eq!(state.graph.nodes.len(), 1);
        assert_eq!(state.registered_patterns, vec!["legacy"]);
        assert_eq!(state.source_index.len(), 1);
        assert_eq!(state.source_index[0].frecency, 7);
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

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].target, "src/lib.rs#fn:run");
        assert_eq!(suggestions[0].kind, "function");
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
                { "path": "src/lib.rs", "frecency": 1 }
              ]
            }"#,
        )
        .unwrap()
    }
}
