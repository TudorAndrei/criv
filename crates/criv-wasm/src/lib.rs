use std::collections::{BTreeSet, HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

const STATE_SCHEMA: &str = "criv.state.v1";
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

    #[wasm_bindgen(js_name = lookupNode)]
    pub fn lookup_node(&self, target: &str) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.prepared.lookup_node(target)).map_err(|error| {
            JsValue::from_str(&format!("failed to encode criv graph node: {error}"))
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
                "invalid criv state JSON: {}",
                js_error_message(&error)
            ))
        })?;
        let schema = js_sys::Reflect::get(&envelope, &JsValue::from_str("schema"))
            .ok()
            .and_then(|value| value.as_string())
            .unwrap_or_else(|| "<missing>".into());
        if schema != STATE_SCHEMA {
            return Err(JsValue::from_str(&format!(
                "unsupported criv state schema: {schema}"
            )));
        }
        let state = serde_wasm_bindgen::from_value::<CrivState>(envelope.clone())
            .map_err(|error| JsValue::from_str(&format!("invalid criv state JSON: {error}")))?;
        let prepared = PreparedState::new(state);
        let initial_projections = initial_projections_from_js(&envelope, &prepared)?;
        Ok(Self {
            #[cfg(not(target_arch = "wasm32"))]
            initial_envelope: None,
            initial_projections: Some(initial_projections),
            prepared,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn prepare_initial_projections(&mut self) -> Result<(), JsValue> {
        let envelope = self
            .initial_envelope
            .as_ref()
            .ok_or_else(|| JsValue::from_str(INITIAL_PROJECTIONS_TAKEN))?;
        let projections = InitialProjections {
            state: envelope,
            summary: &self.prepared.summary,
            sources: &self.prepared.sources,
            nodes: &self.prepared.nodes,
        };
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
        let envelope = decode_state_value(raw)?;
        let state = CrivState::deserialize(&envelope)
            .map_err(|error| format!("invalid criv state JSON: {error}"))?;
        let prepared = PreparedState::new(state);
        Ok(Self {
            initial_envelope: Some(envelope),
            initial_projections: None,
            prepared,
        })
    }

    #[cfg(test)]
    fn take_initial_envelope(&mut self) -> Result<serde_json::Value, String> {
        self.initial_envelope
            .take()
            .ok_or_else(|| INITIAL_PROJECTIONS_TAKEN.to_string())
    }
}

#[cfg(target_arch = "wasm32")]
fn initial_projections_from_js(
    state: &JsValue,
    prepared: &PreparedState,
) -> Result<JsValue, JsValue> {
    let projections = js_sys::Object::new();
    set_js_field(&projections, "state", state)?;
    set_js_field(
        &projections,
        "summary",
        &serde_wasm_bindgen::to_value(&prepared.summary).map_err(js_encode_error)?,
    )?;
    set_js_field(
        &projections,
        "sources",
        &serde_wasm_bindgen::to_value(&prepared.sources).map_err(js_encode_error)?,
    )?;
    set_js_field(
        &projections,
        "nodes",
        &serde_wasm_bindgen::to_value(&prepared.nodes).map_err(js_encode_error)?,
    )?;
    Ok(projections.into())
}

#[cfg(target_arch = "wasm32")]
fn set_js_field(object: &js_sys::Object, name: &str, value: &JsValue) -> Result<(), JsValue> {
    js_sys::Reflect::set(object, &JsValue::from_str(name), value)
        .map(|_| ())
        .map_err(|error| {
            JsValue::from_str(&format!(
                "failed to encode criv initial projections: {}",
                js_error_message(&error)
            ))
        })
}

#[cfg(target_arch = "wasm32")]
fn js_encode_error(error: serde_wasm_bindgen::Error) -> JsValue {
    JsValue::from_str(&format!(
        "failed to encode criv initial projections: {error}"
    ))
}

#[cfg(target_arch = "wasm32")]
fn js_error_message(error: &JsValue) -> String {
    js_sys::Reflect::get(error, &JsValue::from_str("message"))
        .ok()
        .and_then(|value| value.as_string())
        .or_else(|| error.as_string())
        .unwrap_or_else(|| "unknown JavaScript error".into())
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(test)]
fn unique_source_paths(source_index: &[SourceIndexEntry]) -> Vec<String> {
    unique_source_entries(source_index)
        .into_iter()
        .map(|entry| entry.path)
        .collect()
}

#[cfg(test)]
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

fn take_unique_source_entries(source_index: Vec<SourceIndexEntry>) -> Vec<EditorSourceEntry> {
    let mut seen = BTreeSet::new();
    source_index
        .into_iter()
        .filter_map(|entry| {
            let path = safe_source_path(&entry.path)?;
            seen.insert(path.clone()).then_some(EditorSourceEntry {
                path,
                mime: entry.mime,
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

#[cfg(test)]
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

fn take_editor_graph_nodes(nodes: Vec<Node>) -> Vec<EditorGraphNode> {
    nodes
        .into_iter()
        .map(|node| {
            let source_target = source_target(&node);
            let line_range = node.path.as_deref().and_then(line_range);
            let label = node.label.unwrap_or_else(|| node.id.clone());
            EditorGraphNode {
                id: node.id,
                kind: node.kind.unwrap_or_default(),
                label,
                path: node.path,
                source_target,
                line_range,
            }
        })
        .collect()
}

#[cfg(test)]
fn source_selector_suggestions(
    state: &CrivState,
    query: &str,
    limit: usize,
) -> Vec<SourceSelectorSuggestion> {
    PreparedState::from_borrowed(state).suggest_selectors(query, limit)
}

#[cfg(test)]
fn find_editor_graph_node(state: &CrivState, target: &str) -> Option<EditorGraphNode> {
    PreparedState::from_borrowed(state)
        .lookup_node(target)
        .cloned()
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

fn source_match_score_prepared(lower_path: &str, basename: &str, query: &str) -> Option<i64> {
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
    fuzzy_subsequence_score(lower_path, query).map(|score| 40_000 + score - lower_path.len() as i64)
}

impl PreparedState {
    fn new(state: CrivState) -> Self {
        let CrivState {
            schema,
            graph,
            registered_patterns,
            source_index,
        } = state;
        let Graph { nodes, edges } = graph;
        let sources = take_unique_source_entries(source_index);
        let nodes = take_editor_graph_nodes(nodes);
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
        Self::from_parts(summary, sources, nodes)
    }

    #[cfg(test)]
    fn from_borrowed(state: &CrivState) -> Self {
        let sources = unique_source_entries(&state.source_index);
        let nodes = editor_graph_nodes(state);
        let summary = state_summary(state, &sources);
        Self::from_parts(summary, sources, nodes)
    }

    fn from_parts(
        summary: StateSummary,
        sources: Vec<EditorSourceEntry>,
        nodes: Vec<EditorGraphNode>,
    ) -> Self {
        let mut node_lookup = HashMap::<u64, Vec<usize>>::new();
        for (index, node) in nodes.iter().enumerate() {
            for key in [
                Some(node.id.as_str()),
                node.source_target.as_deref(),
                node.path.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                let indexes = node_lookup.entry(target_hash(key)).or_default();
                if indexes.last() != Some(&index) {
                    indexes.push(index);
                }
            }
        }

        let mut seen = BTreeSet::new();
        let mut selectors = Vec::new();
        for (index, source) in sources.iter().enumerate() {
            if !seen.insert(source.path.as_str()) {
                continue;
            }
            selectors.push(PreparedSelector::new(
                SelectorEntry::Source(index),
                source.frecency,
            ));
        }
        for (index, node) in nodes.iter().enumerate() {
            let Some(target) = node.source_target.as_deref() else {
                continue;
            };
            if !target.contains('#') || !seen.insert(target) {
                continue;
            }
            selectors.push(PreparedSelector::new(SelectorEntry::Node(index), 0));
        }
        drop(seen);
        let mut empty_selector_order = (0..selectors.len()).collect::<Vec<_>>();
        empty_selector_order.sort_by(|left, right| {
            selectors[*right]
                .frecency
                .cmp(&selectors[*left].frecency)
                .then_with(|| {
                    selectors[*left]
                        .target(&sources, &nodes)
                        .cmp(selectors[*right].target(&sources, &nodes))
                })
        });

        Self {
            summary,
            sources,
            nodes,
            node_lookup,
            selectors,
            empty_selector_order,
        }
    }

    fn lookup_node(&self, target: &str) -> Option<&EditorGraphNode> {
        self.node_lookup
            .get(&target_hash(target))?
            .iter()
            .filter_map(|index| self.nodes.get(*index))
            .find(|node| node_matches_target(node, target))
    }

    fn suggest_selectors(&self, query: &str, limit: usize) -> Vec<SourceSelectorSuggestion> {
        let clean_query = query.trim().to_lowercase();
        if clean_query.is_empty() {
            return self
                .empty_selector_order
                .iter()
                .take(limit)
                .map(|index| self.selectors[*index].suggestion(&self.sources, &self.nodes))
                .collect();
        }

        let mut scored = self
            .selectors
            .iter()
            .filter_map(|selector| {
                let lower_target = selector.target(&self.sources, &self.nodes).to_lowercase();
                let basename_start = lower_target.rfind('/').map_or(0, |index| index + 1);
                source_match_score_prepared(
                    &lower_target,
                    &lower_target[basename_start..],
                    &clean_query,
                )
                .map(|score| (selector, score + i64::from(selector.frecency)))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|(left, left_score), (right, right_score)| {
            right_score
                .cmp(left_score)
                .then_with(|| right.frecency.cmp(&left.frecency))
                .then_with(|| {
                    left.target(&self.sources, &self.nodes)
                        .cmp(right.target(&self.sources, &self.nodes))
                })
        });
        scored
            .into_iter()
            .take(limit)
            .map(|(selector, _)| selector.suggestion(&self.sources, &self.nodes))
            .collect()
    }
}

impl PreparedSelector {
    fn new(entry: SelectorEntry, frecency: u32) -> Self {
        Self { entry, frecency }
    }

    fn target<'a>(
        &self,
        sources: &'a [EditorSourceEntry],
        nodes: &'a [EditorGraphNode],
    ) -> &'a str {
        match self.entry {
            SelectorEntry::Source(index) => &sources[index].path,
            SelectorEntry::Node(index) => nodes[index].source_target.as_deref().unwrap_or_default(),
        }
    }

    fn suggestion(
        &self,
        sources: &[EditorSourceEntry],
        nodes: &[EditorGraphNode],
    ) -> SourceSelectorSuggestion {
        match self.entry {
            SelectorEntry::Source(index) => {
                let source = &sources[index];
                SourceSelectorSuggestion {
                    target: source.path.clone(),
                    label: source.path.clone(),
                    kind: "file".into(),
                    path: source.path.clone(),
                    detail: "file".into(),
                }
            }
            SelectorEntry::Node(index) => {
                let node = &nodes[index];
                SourceSelectorSuggestion {
                    target: node.source_target.clone().unwrap_or_default(),
                    label: node.label.clone(),
                    kind: node.kind.clone(),
                    path: node.path.clone().unwrap_or_default(),
                    detail: node.id.clone(),
                }
            }
        }
    }
}

fn target_hash(target: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    target.hash(&mut hasher);
    hasher.finish()
}

fn node_matches_target(node: &EditorGraphNode, target: &str) -> bool {
    node.id == target
        || node.source_target.as_deref() == Some(target)
        || node.path.as_deref() == Some(target)
}

#[cfg(test)]
fn state_summary(state: &CrivState, sources: &[EditorSourceEntry]) -> StateSummary {
    StateSummary {
        schema: state.schema.clone(),
        node_count: state.graph.nodes.len(),
        edge_count: state.graph.edges.len(),
        source_count: sources.len(),
        pattern_count: state.registered_patterns.len(),
        first_node_id: state.graph.nodes.first().map(|node| node.id.clone()),
        first_edge: state
            .graph
            .edges
            .first()
            .map(|edge| format!("{}:{}:{}", edge.from, edge.kind, edge.to)),
        first_source_path: sources.first().map(|source| source.path.clone()),
    }
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

#[cfg(not(target_arch = "wasm32"))]
#[derive(Serialize)]
struct InitialProjections<'a> {
    state: &'a serde_json::Value,
    summary: &'a StateSummary,
    sources: &'a [EditorSourceEntry],
    nodes: &'a [EditorGraphNode],
}

struct PreparedState {
    summary: StateSummary,
    sources: Vec<EditorSourceEntry>,
    nodes: Vec<EditorGraphNode>,
    node_lookup: HashMap<u64, Vec<usize>>,
    selectors: Vec<PreparedSelector>,
    empty_selector_order: Vec<usize>,
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
    use super::*;

    #[test]
    fn loaded_state_prepares_one_revision_for_all_operations() {
        let mut loaded =
            LoadedState::load(include_str!("../../../fixtures/state/criv.state.v1.json")).unwrap();

        assert_eq!(loaded.prepared.summary.node_count, 6);
        assert_eq!(loaded.prepared.sources.len(), 1);
        assert_eq!(loaded.prepared.nodes.len(), 6);
        assert_eq!(
            loaded
                .prepared
                .lookup_node("src/lib.rs#fn:run")
                .unwrap()
                .kind,
            "function"
        );
        assert_eq!(
            loaded.prepared.suggest_selectors("run", 10)[0].target,
            "src/lib.rs#fn:run"
        );

        let envelope = loaded.take_initial_envelope().unwrap();
        assert_eq!(envelope["schema"], "criv.state.v1");
        assert!(envelope["patterns"]["ADR-0001/entrypoint"].is_array());
        assert!(loaded.take_initial_envelope().is_err());
    }

    #[test]
    fn parses_shared_state_contract_fixture() {
        let state = serde_json::from_str::<CrivState>(include_str!(
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
    fn validated_state_preserves_host_consumed_fields() {
        let state =
            decode_state_value(include_str!("../../../fixtures/state/criv.state.v1.json")).unwrap();

        assert_eq!(state["schema"], "criv.state.v1");
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

    fn selector_state() -> CrivState {
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
