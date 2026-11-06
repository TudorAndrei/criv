//! Source projection, lookup, and selector ranking.

use std::collections::{BTreeSet, HashMap};

use criv_state_wire::source_identity::{self, SourceIdentity, SourceSelector};
use criv_state_wire::{Node, SourceIndexEntry, node_id};

use super::path::safe_relative_path;
use super::{
    EditorGraphNode, MAX_AMBIGUOUS_SOURCE_CANDIDATES, PreparedState, SourceSelectorSuggestion,
    SourceTargetCandidate, SourceTargetLookupResult, SuggestionKind,
};

pub fn take_unique_source_entries(source_index: Vec<SourceIndexEntry>) -> Vec<SourceIndexEntry> {
    let mut seen = BTreeSet::new();
    source_index
        .into_iter()
        .filter_map(|entry| {
            let path = safe_relative_path(&entry.path)?;
            seen.insert(path.clone()).then_some(SourceIndexEntry {
                path,
                mime: entry.mime,
            })
        })
        .collect()
}

pub fn take_editor_graph_nodes(nodes: Vec<Node>) -> Vec<EditorGraphNode> {
    nodes
        .into_iter()
        .map(|node| {
            let source_target = node_id::source_target(&node.id).map(ToString::to_string);
            let line_range = node
                .path
                .as_deref()
                .and_then(node_id::line_suffix)
                .map(ToString::to_string);
            let label = if node.label.is_empty() {
                node.id.clone()
            } else {
                node.label
            };
            let mut node = EditorGraphNode {
                id: node.id,
                kind: node.kind,
                label,
                path: node.path,
                source_target,
                line_range,
                identity: None,
            };
            node.identity = canonical_source_target(&node)
                .map(SourceIdentity::parse)
                .map(Box::new);
            node
        })
        .collect()
}

/// The lookup keys and selector order prepared once per loaded State.
pub struct SourceLookup {
    exact: HashMap<String, Vec<usize>>,
    legacy: HashMap<String, Vec<usize>>,
    selectors: Vec<SelectorEntry>,
    empty_selector_order: Vec<usize>,
}

#[derive(Clone, Copy)]
enum SelectorEntry {
    Source(usize),
    Node(usize),
}

impl SourceLookup {
    pub fn new(sources: &[SourceIndexEntry], nodes: &[EditorGraphNode]) -> Self {
        let source_paths = sources
            .iter()
            .map(|source| source.path.as_str())
            .collect::<BTreeSet<_>>();
        let mut exact = HashMap::<String, Vec<usize>>::new();
        let mut legacy = HashMap::<String, Vec<usize>>::new();
        let insert = |lookup: &mut HashMap<String, Vec<usize>>, key: &str, index: usize| {
            let indexes = lookup.entry(key.to_string()).or_default();
            if indexes.last() != Some(&index) {
                indexes.push(index);
            }
        };
        for (index, node) in nodes.iter().enumerate() {
            let has_source = node
                .identity
                .as_ref()
                .is_some_and(|identity| source_paths.contains(identity.path()));
            if !has_source {
                continue;
            }
            for key in [Some(node.id.as_str()), canonical_source_target(node)]
                .into_iter()
                .flatten()
            {
                insert(&mut exact, key, index);
            }
            for key in legacy_node_targets(node) {
                insert(&mut legacy, &key, index);
            }
        }

        let mut seen = BTreeSet::new();
        let mut selectors = Vec::new();
        for (index, source) in sources.iter().enumerate() {
            if seen.insert(source.path.as_str()) {
                selectors.push(SelectorEntry::Source(index));
            }
        }
        for (index, node) in nodes.iter().enumerate() {
            let Some(target) = node.source_target.as_deref() else {
                continue;
            };
            let names_symbol = node
                .identity
                .as_ref()
                .is_some_and(|identity| identity.selector().is_some());
            if names_symbol && seen.insert(target) {
                selectors.push(SelectorEntry::Node(index));
            }
        }
        drop(seen);
        let mut empty_selector_order = (0..selectors.len()).collect::<Vec<_>>();
        empty_selector_order.sort_by_key(|index| {
            selectors
                .get(*index)
                .map(|selector| selector.target(sources, nodes))
        });

        Self {
            exact,
            legacy,
            selectors,
            empty_selector_order,
        }
    }
}

impl PreparedState {
    pub(super) fn lookup_source_target(&self, target: &str) -> SourceTargetLookupResult {
        if target.is_empty() {
            return SourceTargetLookupResult::Unresolved;
        }

        if let Some(indexes) = self.lookup.exact.get(target) {
            let result =
                self.lookup_result(indexes, |node| node_matches_exact_target(node, target));
            if !matches!(result, SourceTargetLookupResult::Unresolved) {
                return result;
            }
        }

        let Some(indexes) = self.lookup.legacy.get(target) else {
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
        is_match: impl Fn(&EditorGraphNode) -> bool,
    ) -> SourceTargetLookupResult {
        let mut matched = indexes
            .iter()
            .filter_map(|index| self.nodes.get(*index))
            .filter(|node| is_match(node))
            .filter_map(|node| Some((SourceTargetCandidate::from_node(node)?, node.clone())))
            .collect::<Vec<_>>();
        matched.sort();
        matched.dedup_by(|left, right| left.0 == right.0);

        let total_candidate_count = matched.len();
        if total_candidate_count > 1 {
            return SourceTargetLookupResult::Ambiguous {
                candidates: matched
                    .into_iter()
                    .take(MAX_AMBIGUOUS_SOURCE_CANDIDATES)
                    .map(|(candidate, _)| candidate)
                    .collect(),
                total_candidate_count,
            };
        }
        matched
            .pop()
            .map_or(SourceTargetLookupResult::Unresolved, |(candidate, node)| {
                SourceTargetLookupResult::Resolved {
                    canonical_target: candidate.canonical_target,
                    node,
                }
            })
    }

    pub(super) fn suggest_selectors(
        &self,
        query: &str,
        limit: usize,
    ) -> Vec<SourceSelectorSuggestion> {
        let selectors = &self.lookup.selectors;
        let clean_query = query.trim().to_lowercase();
        if clean_query.is_empty() {
            return self
                .lookup
                .empty_selector_order
                .iter()
                .filter_map(|index| selectors.get(*index))
                .filter_map(|selector| selector.suggestion(&self.sources, &self.nodes))
                .take(limit)
                .collect();
        }

        let target = |selector: &SelectorEntry| selector.target(&self.sources, &self.nodes);
        let mut scored = selectors
            .iter()
            .filter_map(|selector| {
                let lower_target = target(selector).to_lowercase();
                let basename = lower_target.rsplit('/').next().unwrap_or(&lower_target);
                source_match_score(&lower_target, basename, &clean_query)
                    .map(|score| (selector, score))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|(left, left_score), (right, right_score)| {
            right_score
                .cmp(left_score)
                .then_with(|| target(left).cmp(target(right)))
        });
        scored
            .into_iter()
            .filter_map(|(selector, _)| selector.suggestion(&self.sources, &self.nodes))
            .take(limit)
            .collect()
    }
}

impl SelectorEntry {
    fn target<'a>(self, sources: &'a [SourceIndexEntry], nodes: &'a [EditorGraphNode]) -> &'a str {
        match self {
            Self::Source(index) => sources
                .get(index)
                .map_or_else(Default::default, |source| source.path.as_str()),
            Self::Node(index) => nodes
                .get(index)
                .and_then(|node| node.source_target.as_deref())
                .unwrap_or_default(),
        }
    }

    fn suggestion(
        self,
        sources: &[SourceIndexEntry],
        nodes: &[EditorGraphNode],
    ) -> Option<SourceSelectorSuggestion> {
        match self {
            Self::Source(index) => {
                let source = sources.get(index)?;
                Some(SourceSelectorSuggestion {
                    target: source.path.clone(),
                    label: source.path.clone(),
                    kind: SuggestionKind::File,
                    path: source.path.clone(),
                    detail: "file".into(),
                })
            }
            Self::Node(index) => {
                let node = nodes.get(index)?;
                Some(SourceSelectorSuggestion {
                    target: node.source_target.clone().unwrap_or_default(),
                    label: node.label.clone(),
                    kind: SuggestionKind::Node(node.kind),
                    path: node.path.clone().unwrap_or_default(),
                    detail: node.id.clone(),
                })
            }
        }
    }
}

fn source_match_score(lower_path: &str, basename: &str, query: &str) -> Option<i64> {
    let path_length = score_length(lower_path.len());
    if lower_path == query {
        return Some(100_000);
    }
    if basename == query {
        return Some(90_000);
    }
    if lower_path.ends_with(query) {
        return Some(80_000_i64.saturating_sub(path_length));
    }
    if basename.starts_with(query) {
        return Some(70_000_i64.saturating_sub(score_length(basename.len())));
    }
    if let Some(index) = lower_path.find(query) {
        return Some(
            60_000_i64
                .saturating_sub(score_length(index))
                .saturating_sub(path_length),
        );
    }
    fuzzy_subsequence_score(lower_path, query)
        .map(|score| 40_000_i64.saturating_add(score).saturating_sub(path_length))
}

fn score_length(length: usize) -> i64 {
    i64::try_from(length).unwrap_or(i64::MAX)
}

fn node_matches_exact_target(node: &EditorGraphNode, target: &str) -> bool {
    node.id == target || canonical_source_target(node) == Some(target)
}

/// The Source target of a `code:` or `symbol:` node, or else the node path
/// when it names a file without a location.
fn canonical_source_target(node: &EditorGraphNode) -> Option<&str> {
    node.source_target
        .as_deref()
        .or_else(|| node.path.as_deref().filter(|path| !path.contains('#')))
}

fn legacy_node_targets(node: &EditorGraphNode) -> Vec<String> {
    let Some(identity) = &node.identity else {
        return Vec::new();
    };
    let path = identity.path();
    let Some(selector) = identity.selector() else {
        return path
            .rsplit('/')
            .next()
            .map(str::to_string)
            .into_iter()
            .collect();
    };
    let fragment = selector.to_string();
    let mut selectors = Vec::new();
    if let Some(short_name) = fragment.rsplit(':').next() {
        selectors.push(short_name.to_string());
    }
    if !node.label.is_empty() {
        selectors.push(node.label.clone());
    }
    if let Some(selector) = selector.as_elixir()
        && let (Some(name), Some(arity)) = (selector.callable_name(), selector.callable_arity())
    {
        selectors.extend(source_identity::compatibility_aliases(
            name,
            Some(arity),
            selector.owner_identity().module_name(),
        ));
    }
    let alias = |path: &str, selector: &str| {
        SourceIdentity::symbol(path, SourceSelector::opaque(selector)).to_string()
    };
    let mut targets = selectors
        .iter()
        .map(|selector| alias(path, selector))
        .collect::<Vec<_>>();
    if let Some(basename) = path.rsplit('/').next().filter(|name| *name != path) {
        for selector in std::iter::once(&fragment).chain(&selectors) {
            targets.push(alias(basename, selector));
        }
    }
    targets.sort();
    targets.dedup();
    targets
}

fn fuzzy_subsequence_score(value: &str, query: &str) -> Option<i64> {
    let mut query_chars = query.chars();
    let mut current_query = query_chars.next();
    let mut score = 0_i64;
    let mut run = 0_i64;
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
        run = run.saturating_add(1);
        let boundary_bonus = if previous.is_none() || previous == Some('/') {
            8
        } else {
            0
        };
        score = score.saturating_add(run.saturating_mul(3).saturating_add(boundary_bonus));
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
            kind: node.kind,
            label: node.label.clone(),
        })
    }
}
