//! Shared wire contract for published criv State documents.

pub mod source_identity;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Schema identity for the current published State document.
pub const STATE_SCHEMA: &str = "criv.state.v1";

/// Return true when a schema identity is the current State schema.
pub fn is_supported_schema(schema: &str) -> bool {
    schema == STATE_SCHEMA
}

/// The complete serialized State document shared by native and Wasm code.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StateDocument {
    pub schema: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture: Option<LikeC4ArchitectureState>,
    #[serde(default)]
    pub graph: Graph,
    #[serde(default, rename = "registered-patterns")]
    pub registered_patterns: Vec<String>,
    #[serde(default)]
    pub patterns: BTreeMap<String, Vec<PatternMatch>>,
    #[serde(default, rename = "source-index")]
    pub source_index: Vec<SourceIndexEntry>,
    #[serde(default, rename = "asset-index", skip_serializing_if = "Vec::is_empty")]
    pub asset_index: Vec<AssetIndexEntry>,
}

impl StateDocument {
    pub fn new(
        graph: Graph,
        registered_patterns: Vec<String>,
        patterns: BTreeMap<String, Vec<PatternMatch>>,
        source_index: Vec<SourceIndexEntry>,
    ) -> Self {
        Self {
            schema: STATE_SCHEMA.to_string(),
            architecture: None,
            graph,
            registered_patterns,
            patterns,
            source_index,
            asset_index: Vec::new(),
        }
    }
}

impl Default for StateDocument {
    fn default() -> Self {
        Self::new(Graph::default(), Vec::new(), BTreeMap::new(), Vec::new())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeC4ArchitectureState {
    pub protocol_version: u32,
    pub likec4_version: String,
    pub revision: u64,
    pub workspace: String,
    pub model: serde_json::Value,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Graph {
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Node {
    pub id: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: String,
    #[serde(default)]
    pub hash: String,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct PatternMatch {
    pub file: String,
    #[serde(default)]
    pub range: Option<String>,
    #[serde(default)]
    pub captures: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceIndexEntry {
    pub path: String,
    #[serde(default)]
    pub mime: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct AssetIndexEntry {
    pub path: String,
    pub mime: String,
    pub bytes: u64,
    pub hash: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_uses_one_schema_and_wire_row_contract() {
        let mut document = StateDocument::new(
            Graph {
                root: "root-hash".into(),
                nodes: vec![Node {
                    id: "code:src/lib.rs".into(),
                    hash: "node-hash".into(),
                    kind: "code".into(),
                    label: "src/lib.rs (rust)".into(),
                    path: Some("src/lib.rs".into()),
                }],
                edges: vec![Edge {
                    from: "code:src/lib.rs".into(),
                    to: "symbol:src/lib.rs#fn:run".into(),
                    kind: "contains".into(),
                    hash: "edge-hash".into(),
                }],
            },
            vec!["ADR-0001/entrypoint".into()],
            BTreeMap::from([(
                "ADR-0001/entrypoint".into(),
                vec![PatternMatch {
                    file: "src/lib.rs".into(),
                    range: Some("L1-L1".into()),
                    captures: BTreeMap::new(),
                }],
            )]),
            vec![SourceIndexEntry {
                path: "src/lib.rs".into(),
                mime: Some("text/rust".into()),
            }],
        );
        document.asset_index.push(AssetIndexEntry {
            path: "docs/diagram.png".into(),
            mime: "image/png".into(),
            bytes: 128,
            hash: "asset-hash".into(),
        });

        let value = serde_json::to_value(&document).unwrap();
        assert_eq!(value["schema"], STATE_SCHEMA);
        assert_eq!(value["registered-patterns"][0], "ADR-0001/entrypoint");
        assert_eq!(value["source-index"][0]["path"], "src/lib.rs");
        assert_eq!(value["asset-index"][0]["path"], "docs/diagram.png");

        let decoded: StateDocument = serde_json::from_value(value).unwrap();
        assert!(is_supported_schema(&decoded.schema));
        assert_eq!(decoded.graph.nodes[0].hash, "node-hash");
        assert_eq!(decoded.graph.edges[0].hash, "edge-hash");
        assert_eq!(decoded.asset_index[0].hash, "asset-hash");
        assert_eq!(
            decoded.patterns["ADR-0001/entrypoint"][0].file,
            "src/lib.rs"
        );
    }

    #[test]
    fn source_rows_accept_legacy_extra_fields() {
        let entry: SourceIndexEntry =
            serde_json::from_str(r#"{"path":"src/lib.rs","mime":"text/rust","frecency":9}"#)
                .unwrap();
        assert_eq!(entry.path, "src/lib.rs");
        assert_eq!(entry.mime.as_deref(), Some("text/rust"));
        assert_eq!(
            serde_json::to_value(entry).unwrap(),
            serde_json::json!({"path": "src/lib.rs", "mime": "text/rust"})
        );
    }

    #[test]
    fn empty_asset_index_keeps_the_existing_wire_shape() {
        let value = serde_json::to_value(StateDocument::default()).unwrap();
        assert!(value.get("asset-index").is_none());

        let decoded: StateDocument = serde_json::from_str(r#"{"schema":"criv.state.v1"}"#).unwrap();
        assert!(decoded.asset_index.is_empty());
    }
}
