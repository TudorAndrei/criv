use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn summarize_state(raw: &str) -> Result<JsValue, JsValue> {
    let state = serde_json::from_str::<CrivState>(raw)
        .map_err(|err| JsValue::from_str(&format!("invalid criv state JSON: {err}")))?;
    serde_wasm_bindgen::to_value(&StateSummary {
        schema: state.schema,
        node_count: state.graph.nodes.len(),
        edge_count: state.graph.edges.len(),
        source_count: state.source_index.len(),
        pattern_count: state.registered_patterns.len(),
        first_node_id: state.graph.nodes.first().map(|node| node.id.clone()),
        first_edge: state
            .graph
            .edges
            .first()
            .map(|edge| format!("{}:{}:{}", edge.from, edge.kind, edge.to)),
        first_source_path: state.source_index.first().map(|entry| entry.path.clone()),
    })
    .map_err(|err| JsValue::from_str(&format!("failed to encode criv summary: {err}")))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_state_shape() {
        let raw = r#"{
          "schema": "criv.state.v0",
          "graph": { "nodes": [{ "id": "note:x" }], "edges": [] },
          "registered-patterns": ["legacy"],
          "source-index": [{ "path": "src/lib.rs" }]
        }"#;

        let state = serde_json::from_str::<CrivState>(raw).unwrap();
        assert_eq!(state.schema, "criv.state.v0");
        assert_eq!(state.graph.nodes.len(), 1);
        assert_eq!(state.registered_patterns, vec!["legacy"]);
        assert_eq!(state.source_index.len(), 1);
    }
}
