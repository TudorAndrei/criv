//! Source projection, lookup, and selector ranking.

use std::collections::{BTreeSet, HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use criv_state_wire::{Node, SourceIndexEntry};

use super::*;

#[cfg(test)]
pub(super) fn unique_source_paths(source_index: &[SourceIndexEntry]) -> Vec<String> {
    unique_source_entries(source_index)
        .into_iter()
        .map(|entry| entry.path)
        .collect()
}

#[cfg(test)]
pub(super) fn unique_source_entries(source_index: &[SourceIndexEntry]) -> Vec<EditorSourceEntry> {
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

pub(super) fn take_unique_source_entries(
    source_index: Vec<SourceIndexEntry>,
) -> Vec<EditorSourceEntry> {
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

pub(super) fn safe_source_path(path: &str) -> Option<String> {
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
pub(super) fn editor_graph_nodes(state: &StateDocument) -> Vec<EditorGraphNode> {
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

pub(super) fn take_editor_graph_nodes(nodes: Vec<Node>) -> Vec<EditorGraphNode> {
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
pub(super) fn source_selector_suggestions(
    state: &StateDocument,
    query: &str,
    limit: usize,
) -> Vec<SourceSelectorSuggestion> {
    PreparedState::from_borrowed(state).suggest_selectors(query, limit)
}

#[cfg(test)]
pub(super) fn find_editor_graph_node(
    state: &StateDocument,
    target: &str,
) -> Option<EditorGraphNode> {
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
    pub(super) fn from_parts(
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

    pub(super) fn lookup_source_target(&self, target: &str) -> SourceTargetLookupResult {
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

    pub(super) fn suggest_selectors(
        &self,
        query: &str,
        limit: usize,
    ) -> Vec<SourceSelectorSuggestion> {
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
