use std::collections::{BTreeMap, BTreeSet, HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use criv_state_wire::{
    LikeC4ArchitectureState, Node, PatternMatch, SourceIndexEntry, StateDocument,
    is_supported_schema,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

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
                js_error_message(&error)
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
        validate_architecture_wrapper_wasm(&envelope)?;
        let state = serde_wasm_bindgen::from_value::<StateDocument>(envelope)
            .map_err(|error| JsValue::from_str(&format!("criv-state-json-invalid: {error}")))?;
        let prepared = PreparedState::new(state).map_err(|error| JsValue::from_str(&error))?;
        let initial_projections = initial_projections_from_js(&prepared)?;
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
        let envelope = decode_state_value(raw)?;
        validate_architecture_wrapper(&envelope)?;
        let state = StateDocument::deserialize(&envelope)
            .map_err(|error| format!("criv-state-json-invalid: {error}"))?;
        let prepared = PreparedState::new(state)?;
        let initial_envelope = serde_json::to_value(InitialProjections::from(&prepared))
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

#[cfg(target_arch = "wasm32")]
fn validate_architecture_wrapper_wasm(envelope: &JsValue) -> Result<(), JsValue> {
    let architecture = js_sys::Reflect::get(envelope, &JsValue::from_str("architecture"))
        .unwrap_or(JsValue::UNDEFINED);
    if architecture.is_undefined() || architecture.is_null() {
        return Ok(());
    }
    serde_wasm_bindgen::from_value::<LikeC4ArchitectureState>(architecture)
        .map(|_| ())
        .map_err(|error| JsValue::from_str(&format!("criv-likec4-architecture-invalid: {error}")))
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_architecture_wrapper(envelope: &serde_json::Value) -> Result<(), String> {
    let Some(architecture) = envelope
        .get("architecture")
        .filter(|value| !value.is_null())
    else {
        return Ok(());
    };
    serde_json::from_value::<LikeC4ArchitectureState>(architecture.clone())
        .map(|_| ())
        .map_err(|error| format!("criv-likec4-architecture-invalid: {error}"))
}

#[cfg(target_arch = "wasm32")]
fn initial_projections_from_js(prepared: &PreparedState) -> Result<JsValue, JsValue> {
    let projections = js_sys::Object::new();
    set_js_field(
        &projections,
        "summary",
        &js_projection_value(&prepared.summary)?,
    )?;
    set_js_field(
        &projections,
        "sources",
        &js_projection_value(&prepared.sources)?,
    )?;
    set_js_field(
        &projections,
        "nodes",
        &js_projection_value(&prepared.nodes)?,
    )?;
    set_js_field(
        &projections,
        "registeredPatterns",
        &js_projection_value(&prepared.registered_patterns)?,
    )?;
    set_js_field(
        &projections,
        "patternMatches",
        &js_projection_value(&prepared.pattern_matches)?,
    )?;
    set_js_field(
        &projections,
        "architecture",
        &js_projection_value(&prepared.architecture)?,
    )?;
    set_js_field(
        &projections,
        "c4Artifacts",
        &js_projection_value(&prepared.c4_artifacts)?,
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
fn js_projection_value(value: &impl Serialize) -> Result<JsValue, JsValue> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
        .map_err(js_encode_error)
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
        .map_err(|err| format!("criv-state-json-invalid: {err}"))?;
    let schema = state
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>");
    if !is_supported_schema(schema) {
        return Err(format!(
            "criv-state-schema-unsupported: unsupported criv state schema: {schema}"
        ));
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
fn editor_graph_nodes(state: &StateDocument) -> Vec<EditorGraphNode> {
    state
        .graph
        .nodes
        .iter()
        .map(|node| EditorGraphNode {
            id: node.id.clone(),
            kind: node.kind.clone(),
            label: if node.label.is_empty() {
                node.id.clone()
            } else {
                node.label.clone()
            },
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
            let label = if node.label.is_empty() {
                node.id.clone()
            } else {
                node.label
            };
            EditorGraphNode {
                id: node.id,
                kind: node.kind,
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
    state: &StateDocument,
    query: &str,
    limit: usize,
) -> Vec<SourceSelectorSuggestion> {
    PreparedState::from_borrowed(state).suggest_selectors(query, limit)
}

#[cfg(test)]
fn find_editor_graph_node(state: &StateDocument, target: &str) -> Option<EditorGraphNode> {
    match PreparedState::from_borrowed(state).lookup_source_target(target) {
        SourceTargetLookupResult::Resolved { node, .. } => Some(node),
        SourceTargetLookupResult::Unresolved | SourceTargetLookupResult::Ambiguous { .. } => None,
    }
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
        let sources = take_unique_source_entries(source_index);
        let nodes = take_editor_graph_nodes(nodes);
        registered_patterns.sort();
        registered_patterns.dedup();
        let architecture = prepare_architecture(architecture)?;
        let c4_artifacts = prepare_c4_artifacts(&sources, &nodes);
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

    fn from_parts(
        summary: StateSummary,
        sources: Vec<EditorSourceEntry>,
        nodes: Vec<EditorGraphNode>,
        registered_patterns: Vec<String>,
        pattern_matches: BTreeMap<String, Vec<PatternMatch>>,
        architecture: Option<EditorLikeC4Model>,
        c4_artifacts: Vec<EditorC4Artifact>,
    ) -> Self {
        let source_paths = sources
            .iter()
            .map(|source| source.path.as_str())
            .collect::<BTreeSet<_>>();
        let mut exact_source_lookup = HashMap::<u64, Vec<usize>>::new();
        let mut legacy_source_lookup = HashMap::<u64, Vec<usize>>::new();
        for (index, node) in nodes.iter().enumerate() {
            if !node_has_prepared_source(node, &source_paths) {
                continue;
            }
            let exact_keys = [Some(node.id.as_str()), canonical_source_target(node)];
            for key in exact_keys.into_iter().flatten() {
                let indexes = exact_source_lookup.entry(target_hash(key)).or_default();
                if indexes.last() != Some(&index) {
                    indexes.push(index);
                }
            }
            for key in legacy_node_targets(node) {
                let indexes = legacy_source_lookup.entry(target_hash(&key)).or_default();
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
            registered_patterns,
            pattern_matches,
            architecture,
            c4_artifacts,
            exact_source_lookup,
            legacy_source_lookup,
            selectors,
            empty_selector_order,
        }
    }

    fn lookup_source_target(&self, target: &str) -> SourceTargetLookupResult {
        if target.is_empty() || target.contains('\\') {
            return SourceTargetLookupResult::Unresolved;
        }

        if let Some(indexes) = self.exact_source_lookup.get(&target_hash(target)) {
            let result =
                self.lookup_result(indexes, |node| node_matches_exact_target(node, target));
            if !matches!(result, SourceTargetLookupResult::Unresolved) {
                return result;
            }
        }

        let Some(indexes) = self.legacy_source_lookup.get(&target_hash(target)) else {
            return SourceTargetLookupResult::Unresolved;
        };
        self.lookup_result(indexes, |node| {
            legacy_node_targets(node)
                .iter()
                .any(|alias| alias == target)
        })
    }

    fn lookup_result(
        &self,
        indexes: &[usize],
        matches: impl Fn(&EditorGraphNode) -> bool,
    ) -> SourceTargetLookupResult {
        let mut matched = indexes
            .iter()
            .filter_map(|index| self.nodes.get(*index))
            .filter(|node| matches(node))
            .filter_map(|node| Some((SourceTargetCandidate::from_node(node)?, node.clone())))
            .collect::<Vec<_>>();
        matched.sort();
        matched.dedup_by(|left, right| left.0 == right.0);

        match matched.len() {
            0 => SourceTargetLookupResult::Unresolved,
            1 => {
                let (candidate, node) = matched.pop().expect("one lookup candidate");
                SourceTargetLookupResult::Resolved {
                    canonical_target: candidate.canonical_target,
                    node,
                }
            }
            total_candidate_count => SourceTargetLookupResult::Ambiguous {
                candidates: matched
                    .into_iter()
                    .take(MAX_AMBIGUOUS_SOURCE_CANDIDATES)
                    .map(|(candidate, _)| candidate)
                    .collect(),
                total_candidate_count,
            },
        }
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LikeC4Contract {
    protocol_version: u32,
    likec4_version: String,
}

fn prepare_architecture(
    architecture: Option<LikeC4ArchitectureState>,
) -> Result<Option<EditorLikeC4Model>, String> {
    let Some(architecture) = architecture else {
        return Ok(None);
    };
    let contract = serde_json::from_str::<LikeC4Contract>(include_str!(
        "../../../assets/likec4-contract.json"
    ))
    .expect("the embedded LikeC4 contract must be valid JSON");
    if architecture.protocol_version != contract.protocol_version {
        return Err(format!(
            "criv-likec4-protocol-unsupported: expected protocol {}; got {}",
            contract.protocol_version, architecture.protocol_version
        ));
    }
    if architecture.likec4_version != contract.likec4_version {
        return Err(format!(
            "criv-likec4-version-unsupported: expected LikeC4 {}; got {}",
            contract.likec4_version, architecture.likec4_version
        ));
    }
    let workspace = safe_source_path(&architecture.workspace).ok_or_else(|| {
        format!(
            "criv-likec4-architecture-invalid: invalid workspace {}",
            architecture.workspace
        )
    })?;
    let published = serde_json::from_value::<PublishedLikeC4Model>(architecture.model)
        .map_err(|error| format!("criv-likec4-model-invalid: {error}"))?;
    validate_raw_likec4_model(&published.raw)?;
    let mut seen_views = BTreeSet::new();
    for view in &published.views {
        if view.id.trim().is_empty()
            || view.title.trim().is_empty()
            || !seen_views.insert(view.id.as_str())
            || view
                .source_path
                .as_deref()
                .is_some_and(|path| safe_source_path(path).is_none())
        {
            return Err("criv-likec4-model-invalid: invalid named view".into());
        }
    }
    for link in &published.source_links {
        let path = link
            .target
            .split_once('#')
            .map_or(link.target.as_str(), |value| value.0);
        if link.element.trim().is_empty() || safe_source_path(path).is_none() {
            return Err("criv-likec4-model-invalid: invalid source link".into());
        }
    }
    Ok(Some(EditorLikeC4Model {
        protocol_version: contract.protocol_version,
        likec4_version: contract.likec4_version,
        workspace,
        model: published.raw,
        views: published.views,
        source_links: published.source_links,
    }))
}

fn validate_raw_likec4_model(raw: &serde_json::Value) -> Result<(), String> {
    let Some(raw) = raw.as_object() else {
        return Err("criv-likec4-model-invalid: raw model must be an object".into());
    };
    if raw.get("_stage").and_then(serde_json::Value::as_str) != Some("layouted")
        || !raw
            .get("projectId")
            .is_some_and(serde_json::Value::is_string)
    {
        return Err("criv-likec4-model-invalid: raw model must identify a layouted project".into());
    }
    for field in [
        "project",
        "specification",
        "elements",
        "relations",
        "globals",
        "views",
        "imports",
        "manualLayouts",
    ] {
        if !raw.get(field).is_some_and(serde_json::Value::is_object) {
            return Err(format!(
                "criv-likec4-model-invalid: raw model field {field} must be an object"
            ));
        }
    }
    let deployments = raw
        .get("deployments")
        .and_then(serde_json::Value::as_object);
    if !deployments
        .and_then(|value| value.get("elements"))
        .is_some_and(serde_json::Value::is_object)
        || !deployments
            .and_then(|value| value.get("relations"))
            .is_some_and(serde_json::Value::is_object)
    {
        return Err(
            "criv-likec4-model-invalid: raw model deployments must contain elements and relations"
                .into(),
        );
    }
    Ok(())
}

fn prepare_c4_artifacts(
    sources: &[EditorSourceEntry],
    nodes: &[EditorGraphNode],
) -> Vec<EditorC4Artifact> {
    let mut artifacts = BTreeMap::<String, EditorC4Artifact>::new();
    for source in sources {
        if is_c4_path(&source.path) {
            artifacts.insert(
                source.path.clone(),
                EditorC4Artifact {
                    path: source.path.clone(),
                    label: source.path.clone(),
                    target: source.path.clone(),
                },
            );
        }
    }
    for node in nodes {
        let Some(target) = node.source_target.as_deref().or(node.path.as_deref()) else {
            continue;
        };
        let path = node.path.as_deref().unwrap_or(target);
        if !is_c4_path(path) {
            continue;
        }
        artifacts.insert(
            path.to_string(),
            EditorC4Artifact {
                path: path.to_string(),
                label: if node.label.is_empty() {
                    path.to_string()
                } else {
                    node.label.clone()
                },
                target: target.to_string(),
            },
        );
    }
    artifacts.into_values().collect()
}

fn is_c4_path(path: &str) -> bool {
    path.split_once('#')
        .map_or(path, |value| value.0)
        .ends_with(".c4")
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

fn node_matches_exact_target(node: &EditorGraphNode, target: &str) -> bool {
    node.id == target || canonical_source_target(node) == Some(target)
}

fn canonical_source_target(node: &EditorGraphNode) -> Option<&str> {
    node.source_target.as_deref().or_else(|| {
        node.path
            .as_deref()
            .filter(|path| !path.contains("#L") && !path.contains("#l"))
    })
}

fn node_has_prepared_source(node: &EditorGraphNode, source_paths: &BTreeSet<&str>) -> bool {
    let Some(target) = canonical_source_target(node) else {
        return false;
    };
    let path = target.split_once('#').map_or(target, |(path, _)| path);
    !path.contains('\\')
        && safe_source_path(path).is_some_and(|path| source_paths.contains(path.as_str()))
}

fn legacy_node_targets(node: &EditorGraphNode) -> Vec<String> {
    let mut targets = Vec::new();
    if let Some(source_target) = &node.source_target {
        if let Some((path, fragment)) = source_target.split_once('#') {
            if let Some(short_name) = fragment.rsplit(':').next() {
                targets.push(format!("{path}#{short_name}"));
            }
            if !node.label.is_empty() {
                targets.push(format!("{path}#{}", node.label));
            }
        } else if let Some(basename) = source_target.rsplit('/').next() {
            targets.push(basename.to_string());
        }
    } else if let Some(path) = node.path.as_deref().filter(|path| !path.contains('#'))
        && let Some(basename) = path.rsplit('/').next()
    {
        targets.push(basename.to_string());
    }
    targets.sort();
    targets.dedup();
    targets
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
    summary: &'a StateSummary,
    sources: &'a [EditorSourceEntry],
    nodes: &'a [EditorGraphNode],
    #[serde(rename = "registeredPatterns")]
    registered_patterns: &'a [String],
    #[serde(rename = "patternMatches")]
    pattern_matches: &'a BTreeMap<String, Vec<PatternMatch>>,
    architecture: &'a Option<EditorLikeC4Model>,
    #[serde(rename = "c4Artifacts")]
    c4_artifacts: &'a [EditorC4Artifact],
}

#[cfg(not(target_arch = "wasm32"))]
impl<'a> From<&'a PreparedState> for InitialProjections<'a> {
    fn from(prepared: &'a PreparedState) -> Self {
        Self {
            summary: &prepared.summary,
            sources: &prepared.sources,
            nodes: &prepared.nodes,
            registered_patterns: &prepared.registered_patterns,
            pattern_matches: &prepared.pattern_matches,
            architecture: &prepared.architecture,
            c4_artifacts: &prepared.c4_artifacts,
        }
    }
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

impl SourceTargetCandidate {
    fn from_node(node: &EditorGraphNode) -> Option<Self> {
        Some(Self {
            canonical_target: canonical_source_target(node)?.to_string(),
            node_id: node.id.clone(),
            kind: node.kind.clone(),
            label: node.label.clone(),
        })
    }
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
