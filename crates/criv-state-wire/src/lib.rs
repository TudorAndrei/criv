//! Shared wire contract for published criv State documents.

pub mod asset;
pub mod content_hash;
pub mod layout;
pub mod node_id;
pub mod source_identity;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Schema identity for the current published State document.
pub const STATE_SCHEMA: &str = "criv.state.v1";

/// The complete serialized State document shared by native and Wasm code.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StateDocument {
    pub schema: String,
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
    #[must_use]
    pub fn new(
        graph: Graph,
        registered_patterns: Vec<String>,
        patterns: BTreeMap<String, Vec<PatternMatch>>,
        source_index: Vec<SourceIndexEntry>,
    ) -> Self {
        Self {
            schema: STATE_SCHEMA.to_string(),
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

/// Why a State document text cannot be read.
#[derive(Debug)]
pub enum StateReadError {
    Json(serde_json::Error),
    /// The `schema` value found, or `<missing>`.
    UnsupportedSchema(String),
}

#[derive(Deserialize)]
struct SchemaView {
    #[serde(default)]
    schema: Option<serde_json::Value>,
}

/// Checks that `raw` is a JSON object whose `schema` is [`STATE_SCHEMA`],
/// without decoding the graph, so a revision with a kind this build does not
/// know still passes.
///
/// # Errors
///
/// Returns an error when `raw` is not JSON or names another schema.
pub fn check_schema(raw: &str) -> Result<(), StateReadError> {
    let view = serde_json::from_str::<SchemaView>(raw).map_err(StateReadError::Json)?;
    match view.schema.as_ref().and_then(serde_json::Value::as_str) {
        Some(STATE_SCHEMA) => Ok(()),
        found => Err(StateReadError::UnsupportedSchema(
            found.unwrap_or("<missing>").to_string(),
        )),
    }
}

/// Decodes a State document after [`check_schema`].
///
/// # Errors
///
/// Returns an error when the schema check fails or the document does not
/// match the State grammar.
pub fn decode(raw: &str) -> Result<StateDocument, StateReadError> {
    check_schema(raw)?;
    serde_json::from_str(raw).map_err(StateReadError::Json)
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
    pub kind: NodeKind,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub path: Option<String>,
}

/// Kind of a graph node. Serde and `as_str` share one kebab-case spelling.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize,
    strum::Display,
    strum::IntoStaticStr,
)]
#[cfg_attr(test, derive(strum::EnumIter))]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum NodeKind {
    Alias,
    Behaviour,
    Callback,
    Class,
    Code,
    Decision,
    Doc,
    DocHeading,
    DynamicCall,
    Exception,
    ExternalCall,
    ExternalModule,
    Function,
    Guard,
    Implementation,
    Import,
    Macro,
    MacroCallback,
    Method,
    Module,
    Pattern,
    Protocol,
    Require,
    Struct,
    Use,
}

impl NodeKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    #[serde(default)]
    pub hash: String,
}

/// The `{from}:{kind}:{to}` text of an edge.
fn edge_row(from: &str, kind: &str, to: &str) -> String {
    format!("{from}:{kind}:{to}")
}

/// The node ids and edge rows of a State document read without its schema
/// or kind types, so a revision with a kind this build does not know still
/// compares.
#[derive(Debug, Default, Deserialize)]
pub struct GraphRowsView {
    #[serde(default)]
    graph: GraphRowsViewGraph,
}

#[derive(Debug, Default, Deserialize)]
struct GraphRowsViewGraph {
    #[serde(default)]
    nodes: Vec<NodeIdView>,
    #[serde(default)]
    edges: Vec<EdgeTextView>,
}

#[derive(Debug, Deserialize)]
struct NodeIdView {
    id: String,
}

#[derive(Debug, Deserialize)]
struct EdgeTextView {
    from: String,
    kind: String,
    to: String,
}

impl GraphRowsView {
    #[must_use]
    pub fn node_ids(&self) -> BTreeSet<&str> {
        self.graph
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect()
    }

    #[must_use]
    pub fn edge_rows(&self) -> BTreeSet<String> {
        self.graph
            .edges
            .iter()
            .map(|edge| edge_row(&edge.from, &edge.kind, &edge.to))
            .collect()
    }
}

/// Kind of a graph edge. Serde and `as_str` share one kebab-case spelling.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize,
    strum::Display,
    strum::IntoStaticStr,
)]
#[cfg_attr(test, derive(strum::EnumIter))]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum EdgeKind {
    Calls,
    Captures,
    Cites,
    Contains,
    Delegates,
    Governs,
    ImplementsBehaviour,
    ImplementsFor,
    ImplementsProtocol,
    Imports,
    ProtocolImplementation,
    References,
    Supersedes,
}

impl EdgeKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.into()
    }
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
                    kind: NodeKind::Code,
                    label: "src/lib.rs (rust)".into(),
                    path: Some("src/lib.rs".into()),
                }],
                edges: vec![Edge {
                    from: "code:src/lib.rs".into(),
                    to: "symbol:src/lib.rs#fn:run".into(),
                    kind: EdgeKind::Contains,
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
    }

    #[test]
    fn kind_hash_names_match_wire_names() {
        use strum::IntoEnumIterator;

        for kind in NodeKind::iter() {
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
        }
        for kind in EdgeKind::iter() {
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
        }
    }

    #[test]
    fn node_kinds_match_the_editor_fixture() {
        use strum::IntoEnumIterator;

        let editor_kinds: Vec<String> =
            serde_json::from_str(include_str!("../../../fixtures/editor/node-kinds.v1.json"))
                .unwrap();
        let wire_kinds: Vec<&str> = NodeKind::iter().map(NodeKind::as_str).collect();
        assert_eq!(wire_kinds, editor_kinds);
    }

    #[test]
    fn kinds_sort_like_their_wire_names() {
        use strum::IntoEnumIterator;

        for left in NodeKind::iter() {
            for right in NodeKind::iter() {
                assert_eq!(left.cmp(&right), left.as_str().cmp(right.as_str()));
            }
        }
        for left in EdgeKind::iter() {
            for right in EdgeKind::iter() {
                assert_eq!(left.cmp(&right), left.as_str().cmp(right.as_str()));
            }
        }
    }

    #[test]
    fn rows_view_reads_unknown_kinds_as_row_text() {
        let view: GraphRowsView = serde_json::from_value(serde_json::json!({
            "graph": {
                "nodes": [{"id": "note:ADR-0001", "kind": "retired-kind"}],
                "edges": [
                    {"from": "note:ADR-0001", "kind": "references", "to": "code:src/lib.rs"},
                    {"from": "a", "kind": "retired-kind", "to": "b"}
                ]
            }
        }))
        .unwrap();

        assert_eq!(view.node_ids(), BTreeSet::from(["note:ADR-0001"]));
        assert_eq!(
            view.edge_rows(),
            BTreeSet::from([
                "note:ADR-0001:references:code:src/lib.rs".to_string(),
                "a:retired-kind:b".to_string()
            ])
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
        assert_eq!(decoded.asset_index, Vec::new());
    }

    #[test]
    fn schema_check_ignores_unknown_kinds_that_decoding_rejects() {
        let future_kind =
            r#"{"schema":"criv.state.v1","graph":{"nodes":[{"id":"x","kind":"future"}]}}"#;
        assert!(check_schema(future_kind).is_ok());
        assert!(matches!(decode(future_kind), Err(StateReadError::Json(_))));

        assert!(matches!(
            decode(r#"{"schema":"criv.state.v0"}"#),
            Err(StateReadError::UnsupportedSchema(schema)) if schema == "criv.state.v0"
        ));
        assert!(matches!(
            check_schema("{}"),
            Err(StateReadError::UnsupportedSchema(schema)) if schema == "<missing>"
        ));
        assert!(matches!(check_schema("{"), Err(StateReadError::Json(_))));
    }
}
