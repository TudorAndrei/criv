use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[cfg(test)]
use std::{cell::Cell, thread_local};

use serde::Serialize;

use crate::source_graph::{Language, SymbolKind};
use crate::structural;
use crate::util::{write_atomic_if_changed_in, write_atomic_in};
use crate::vault::{NoteKind, ResolvedLink, SourceTargetResolution, Vault};
use crate::{CrivError, Result};

const STATE_SCHEMA: &str = "criv.state.v0";

#[derive(Debug, Clone, Serialize)]
pub(crate) struct State {
    schema: &'static str,
    graph: Graph,
    #[serde(rename = "registered-patterns")]
    registered_patterns: Vec<String>,
    patterns: BTreeMap<String, Vec<PatternMatch>>,
    #[serde(rename = "source-index")]
    source_index: Vec<SourceIndexEntry>,
}

struct SerializedState {
    published: String,
    hash: String,
}

#[cfg(test)]
thread_local! {
    static BUILD_COUNT: Cell<usize> = const { Cell::new(0) };
    static SERIALIZATION_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct Graph {
    root: String,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Node {
    id: String,
    hash: String,
    kind: String,
    label: String,
    path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Edge {
    from: String,
    to: String,
    kind: String,
    hash: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct PatternMatch {
    file: String,
    range: Option<String>,
    captures: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SourceIndexEntry {
    path: String,
    mime: Option<String>,
    frecency: u32,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct C4InterfaceHashRecord {
    pub(crate) id: String,
    pub(crate) hash: String,
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) target: String,
}

impl State {
    pub(crate) fn build(root: &Path, vault: &Vault) -> Result<Self> {
        Self::build_incremental(root, vault, None, &[])
    }

    fn build_incremental(
        root: &Path,
        vault: &Vault,
        previous: Option<&State>,
        changed_files: &[String],
    ) -> Result<Self> {
        #[cfg(test)]
        BUILD_COUNT.with(|count| count.set(count.get() + 1));

        let mut graph = Graph::default();
        let mut seen_nodes = BTreeSet::new();
        let mut seen_edges = BTreeSet::new();

        for file in vault.source_graph().files.values() {
            add_node(
                &mut graph,
                &mut seen_nodes,
                Node {
                    id: code_node_id(&file.path),
                    hash: String::new(),
                    kind: "code".into(),
                    label: format!("{} ({})", file.path, language_name(file.language)),
                    path: Some(file.path.clone()),
                },
            );
        }

        for file in vault.source_graph().files.values() {
            let file_id = code_node_id(&file.path);
            for import in &file.imports {
                let import_id = import_node_id(&file.path, &import.module);
                add_node(
                    &mut graph,
                    &mut seen_nodes,
                    Node {
                        id: import_id.clone(),
                        hash: String::new(),
                        kind: "import".into(),
                        label: import.module.clone(),
                        path: Some(format!("{}#L{}", file.path, import.line)),
                    },
                );
                add_edge(&mut graph, &mut seen_edges, &file_id, &import_id, "imports");
            }

            for symbol in &file.symbols {
                let symbol_id = symbol_node_id(&symbol.id.display());
                add_node(
                    &mut graph,
                    &mut seen_nodes,
                    Node {
                        id: symbol_id.clone(),
                        hash: String::new(),
                        kind: symbol_kind(symbol.kind).into(),
                        label: symbol.name.clone(),
                        path: Some(format!(
                            "{}#L{}-L{}",
                            symbol.id.path, symbol.range.start_line, symbol.range.end_line
                        )),
                    },
                );
                add_edge(
                    &mut graph,
                    &mut seen_edges,
                    &file_id,
                    &symbol_id,
                    "contains",
                );
                if let Some(parent) = &symbol.parent
                    && let Some(parent_id) = vault
                        .source_graph()
                        .resolve_symbol(&format!("{}#{}", symbol.id.path, parent))
                {
                    add_edge(
                        &mut graph,
                        &mut seen_edges,
                        &symbol_node_id(&parent_id.display()),
                        &symbol_id,
                        "contains",
                    );
                }
                for call in &symbol.calls {
                    let target = vault
                        .source_graph()
                        .resolve_call(&symbol.id, &call.target)
                        .map(|target| symbol_node_id(&target.display()))
                        .unwrap_or_else(|| external_call_node_id(&call.target));
                    if target.starts_with("external-call:") {
                        add_node(
                            &mut graph,
                            &mut seen_nodes,
                            Node {
                                id: target.clone(),
                                hash: String::new(),
                                kind: "external-call".into(),
                                label: call.target.clone(),
                                path: Some(format!("{}#L{}", symbol.id.path, call.line)),
                            },
                        );
                    }
                    add_edge(&mut graph, &mut seen_edges, &symbol_id, &target, "calls");
                }
            }
        }

        for note in &vault.notes {
            let kind = match note.kind {
                NoteKind::Decision => "decision",
                NoteKind::Doc | NoteKind::Unknown => "doc",
            };
            let note_id = note_node_id(note.display_id());
            add_node(
                &mut graph,
                &mut seen_nodes,
                Node {
                    id: note_id.clone(),
                    hash: String::new(),
                    kind: kind.into(),
                    label: note
                        .title
                        .clone()
                        .unwrap_or_else(|| note.display_id().to_string()),
                    path: Some(note.rel_path.clone()),
                },
            );

            for target in &note.targets_symbols {
                if let SourceTargetResolution::Resolved { path, .. } =
                    vault.resolve_source_target(target)
                {
                    add_edge(
                        &mut graph,
                        &mut seen_edges,
                        &note_id,
                        &code_node_id(&path),
                        "references",
                    );
                }
            }

            for heading in &note.headings {
                let heading_id = format!("{note_id}#{}", crate::util::kebab(&heading.text));
                add_node(
                    &mut graph,
                    &mut seen_nodes,
                    Node {
                        id: heading_id.clone(),
                        hash: String::new(),
                        kind: "doc-heading".into(),
                        label: heading.text.clone(),
                        path: Some(format!(
                            "{}#L{}:H{}",
                            note.rel_path, heading.line, heading.level
                        )),
                    },
                );
                add_edge(
                    &mut graph,
                    &mut seen_edges,
                    &note_id,
                    &heading_id,
                    "contains",
                );
            }

            for source_file in vault.source_files_matching_globs(&vault.effective_governs(note)) {
                add_edge(
                    &mut graph,
                    &mut seen_edges,
                    &note_id,
                    &code_node_id(&source_file),
                    "governs",
                );
            }

            for superseded in &note.supersedes {
                add_edge(
                    &mut graph,
                    &mut seen_edges,
                    &note_id,
                    &note_node_id(superseded),
                    "supersedes",
                );
            }

            for link in &note.wiki_links {
                match vault.resolve_link(&link.target) {
                    ResolvedLink::Note { id } => {
                        add_edge(
                            &mut graph,
                            &mut seen_edges,
                            &note_id,
                            &note_node_id(&id),
                            "cites",
                        );
                    }
                    ResolvedLink::Source { path, .. } => {
                        add_edge(
                            &mut graph,
                            &mut seen_edges,
                            &note_id,
                            &code_node_id(&path),
                            "references",
                        );
                    }
                    ResolvedLink::Pattern { id } => {
                        let pattern_id = pattern_node_id(&id);
                        add_node(
                            &mut graph,
                            &mut seen_nodes,
                            Node {
                                id: pattern_id.clone(),
                                hash: String::new(),
                                kind: "pattern".into(),
                                label: id,
                                path: None,
                            },
                        );
                        add_edge(
                            &mut graph,
                            &mut seen_edges,
                            &note_id,
                            &pattern_id,
                            "references",
                        );
                    }
                    ResolvedLink::Broken => {}
                }
            }

            add_c4_diagrams_to_graph(
                &mut graph,
                &mut seen_nodes,
                &mut seen_edges,
                vault,
                &note_id,
                &note.rel_path,
                &note.c4_diagrams,
            );
        }

        for artifact in &vault.c4_artifacts {
            let artifact_id = c4_artifact_node_id(&artifact.rel_path);
            add_node(
                &mut graph,
                &mut seen_nodes,
                Node {
                    id: artifact_id.clone(),
                    hash: String::new(),
                    kind: "c4-artifact".into(),
                    label: artifact.rel_path.clone(),
                    path: Some(artifact.rel_path.clone()),
                },
            );
            add_c4_diagrams_to_graph(
                &mut graph,
                &mut seen_nodes,
                &mut seen_edges,
                vault,
                &artifact_id,
                &artifact.rel_path,
                &artifact.diagrams,
            );
        }

        let mut source_index = vault
            .source_index()
            .entries()?
            .into_iter()
            .map(|entry| SourceIndexEntry {
                mime: mime_guess::from_path(&entry.path)
                    .first_raw()
                    .map(str::to_string),
                path: entry.path,
                frecency: entry.frecency,
            })
            .collect::<Vec<_>>();
        source_index.sort_by(|left, right| left.path.cmp(&right.path));

        let mut patterns = BTreeMap::new();
        for pattern_id in vault.patterns() {
            patterns.insert(
                pattern_id.clone(),
                incremental_pattern_matches(root, vault, previous, changed_files, pattern_id)?,
            );
        }
        graph.root = graph_root(&graph);

        Ok(Self {
            schema: STATE_SCHEMA,
            graph,
            registered_patterns: vault.patterns().iter().cloned().collect(),
            patterns,
            source_index,
        })
    }

    #[cfg(test)]
    fn to_json(&self) -> Result<String> {
        Ok(self
            .serialize()?
            .published
            .strip_suffix('\n')
            .unwrap_or_default()
            .to_string())
    }

    #[cfg(test)]
    fn hash(&self) -> Result<String> {
        Ok(self.serialize()?.hash)
    }

    fn serialize(&self) -> Result<SerializedState> {
        #[cfg(test)]
        SERIALIZATION_COUNT.with(|count| count.set(count.get() + 1));

        let json = serde_json::to_string_pretty(self)
            .map_err(|err| CrivError::new(format!("failed to serialize state: {err}")))?;
        Ok(SerializedState {
            hash: stable_hash(&json),
            published: format!("{json}\n"),
        })
    }

    fn write_serialized(&self, root: &Path, serialized: &SerializedState) -> Result<()> {
        write_atomic_in(
            root,
            Path::new(".criv"),
            Path::new(".criv/state.json"),
            &serialized.published,
        )
    }

    fn write_snapshot_serialized(
        &self,
        root: &Path,
        serialized: &SerializedState,
    ) -> Result<String> {
        let snapshot = format!(".criv/snapshots/{}.json", serialized.hash);
        write_atomic_if_changed_in(
            root,
            Path::new(".criv"),
            Path::new(&snapshot),
            &serialized.published,
        )?;
        write_atomic_in(
            root,
            Path::new(".criv"),
            Path::new(".criv/latest"),
            &format!("{}\n", serialized.hash),
        )?;
        Ok(serialized.hash.clone())
    }
}

pub(crate) fn write_state(root: &Path, vault: &Vault) -> Result<(String, State)> {
    let state = State::build(root, vault)?;
    let serialized = state.serialize()?;
    state.write_serialized(root, &serialized)?;
    let snapshot = state.write_snapshot_serialized(root, &serialized)?;
    Ok((snapshot, state))
}

pub(crate) fn write_state_incremental(
    root: &Path,
    vault: &Vault,
    previous: Option<&State>,
    changed_files: &[String],
) -> Result<(String, State)> {
    let state = State::build_incremental(root, vault, previous, changed_files)?;
    let serialized = state.serialize()?;
    state.write_serialized(root, &serialized)?;
    let snapshot = state.write_snapshot_serialized(root, &serialized)?;
    Ok((snapshot, state))
}

#[cfg(test)]
pub(crate) fn reset_work_counts() {
    BUILD_COUNT.with(|count| count.set(0));
    SERIALIZATION_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn work_counts() -> (usize, usize) {
    (
        BUILD_COUNT.with(Cell::get),
        SERIALIZATION_COUNT.with(Cell::get),
    )
}

fn incremental_pattern_matches(
    root: &Path,
    vault: &Vault,
    previous: Option<&State>,
    changed_files: &[String],
    pattern_id: &str,
) -> Result<Vec<PatternMatch>> {
    let Some(previous_matches) = previous.and_then(|state| state.patterns.get(pattern_id)) else {
        return state_pattern_matches(root, vault, pattern_id, &[]);
    };
    if changed_files.is_empty() {
        return Ok(previous_matches.clone());
    }

    let changed_set = changed_files.iter().cloned().collect::<BTreeSet<_>>();
    let mut matches = previous_matches
        .iter()
        .filter(|matched| !changed_set.contains(&matched.file))
        .cloned()
        .collect::<Vec<_>>();
    matches.extend(state_pattern_matches(
        root,
        vault,
        pattern_id,
        changed_files,
    )?);
    matches.sort_by(|left, right| {
        (&left.file, &left.range, &left.captures).cmp(&(&right.file, &right.range, &right.captures))
    });
    matches.dedup();
    Ok(matches)
}

fn state_pattern_matches(
    root: &Path,
    vault: &Vault,
    pattern_id: &str,
    paths: &[String],
) -> Result<Vec<PatternMatch>> {
    let matches = if let Some((adr_id, local_id)) = pattern_id.split_once('/') {
        let note = vault.resolve_note(adr_id);
        let scopes = note
            .map(|note| vault.effective_governs(note))
            .unwrap_or_else(|| vec!["**".into()]);
        let scoped_paths = scoped_changed_paths(paths, &scopes);
        if let Some(policy) = note.and_then(|note| {
            note.policy_patterns
                .iter()
                .find(|policy| policy.id.as_deref() == Some(local_id))
        }) {
            structural::find_policy_pattern_entry(
                root,
                vault,
                policy,
                structural::PathScope::Globs(&scoped_paths),
            )?
        } else if note.is_some() {
            Vec::new()
        } else if let Some(configured) = vault.config.pattern_defs.get(pattern_id) {
            if let Some(source) = structural::pattern_source(configured) {
                let scoped_paths = if paths.is_empty() {
                    scoped_paths
                } else {
                    let Some(scoped_paths) =
                        configured_pattern_paths(&scoped_paths, configured.language.as_deref())
                    else {
                        return Ok(Vec::new());
                    };
                    scoped_paths
                };
                structural::find(
                    root,
                    vault,
                    source,
                    structural::PathScope::Globs(&scoped_paths),
                    configured.language.as_deref(),
                )?
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else if vault.config.pattern_defs.contains_key(pattern_id) {
        let pattern = &vault.config.pattern_defs[pattern_id];
        if paths.is_empty() {
            structural::find_pattern_id(root, vault, pattern_id, structural::PathScope::All)?
        } else if let Some(source) = structural::pattern_source(pattern) {
            let Some(paths) = configured_pattern_paths(paths, pattern.language.as_deref()) else {
                return Ok(Vec::new());
            };
            structural::find(
                root,
                vault,
                source,
                structural::PathScope::Globs(&paths),
                pattern.language.as_deref(),
            )?
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    Ok(matches
        .into_iter()
        .map(|matched| PatternMatch {
            file: matched.path,
            range: Some(matched.range),
            captures: matched.captures,
        })
        .collect())
}

fn scoped_changed_paths(paths: &[String], scopes: &[String]) -> Vec<String> {
    if paths.is_empty() {
        return scopes.to_vec();
    }
    let matcher = crate::util::GlobMatcher::from_valid_patterns(scopes);
    paths
        .iter()
        .filter(|path| matcher.is_match(path))
        .cloned()
        .collect()
}

fn configured_pattern_paths(paths: &[String], language: Option<&str>) -> Option<Vec<String>> {
    if paths.is_empty() {
        return Some(Vec::new());
    }
    let Some(language) = language else {
        return Some(paths.to_vec());
    };
    let language_glob = structural::language_glob(language);
    let matcher = crate::util::GlobMatcher::from_valid_patterns(&[language_glob.to_string()]);
    let paths = paths
        .iter()
        .filter(|path| matcher.is_match(path))
        .cloned()
        .collect::<Vec<_>>();
    (!paths.is_empty()).then_some(paths)
}

fn add_c4_diagrams_to_graph(
    graph: &mut Graph,
    seen_nodes: &mut BTreeSet<String>,
    seen_edges: &mut BTreeSet<String>,
    vault: &Vault,
    owner_id: &str,
    owner_path: &str,
    diagrams: &[crate::c4::C4Diagram],
) {
    for diagram in diagrams {
        let diagram_id = c4_diagram_node_id(owner_id, diagram.line);
        add_node(
            graph,
            seen_nodes,
            Node {
                id: diagram_id.clone(),
                hash: String::new(),
                kind: "c4-diagram".into(),
                label: format!("{} diagram", diagram.level.as_str()),
                path: Some(format!("{owner_path}#L{}", diagram.line)),
            },
        );
        add_edge(graph, seen_edges, owner_id, &diagram_id, "contains");

        let mut element_nodes = BTreeMap::new();
        for element in &diagram.elements {
            let element_id = c4_element_node_id(owner_id, diagram.line, &element.alias);
            element_nodes
                .entry(element.alias.as_str())
                .or_insert_with(|| element_id.clone());
            add_node(
                graph,
                seen_nodes,
                Node {
                    id: element_id.clone(),
                    hash: String::new(),
                    kind: format!("c4-{}", element.category.as_str()),
                    label: if element.label.is_empty() {
                        element.alias.clone()
                    } else {
                        element.label.clone()
                    },
                    path: Some(format!("{owner_path}#L{}", element.line)),
                },
            );
            add_edge(graph, seen_edges, owner_id, &element_id, "contains");
            add_edge(graph, seen_edges, &diagram_id, &element_id, "contains");
            if let Some(source) = &element.source
                && let SourceTargetResolution::Resolved { path, .. } =
                    vault.resolve_source_target(source)
            {
                add_edge(
                    graph,
                    seen_edges,
                    &element_id,
                    &code_node_id(&path),
                    "references",
                );
                if let Some((target, interface_hash)) = interface_anchor_hash(vault, source, &path)
                {
                    let interface_id = c4_interface_node_id(&element_id);
                    add_node(
                        graph,
                        seen_nodes,
                        Node {
                            id: interface_id.clone(),
                            hash: String::new(),
                            kind: "c4-interface".into(),
                            label: interface_hash,
                            path: Some(target),
                        },
                    );
                    add_edge(
                        graph,
                        seen_edges,
                        &element_id,
                        &interface_id,
                        "tracks-interface",
                    );
                }
            }
        }

        for relationship in &diagram.relationships {
            if let (Some(from), Some(to)) = (
                element_nodes.get(relationship.from.as_str()),
                element_nodes.get(relationship.to.as_str()),
            ) {
                let relationship_id = c4_relationship_node_id(
                    owner_id,
                    diagram.line,
                    relationship.line,
                    &relationship.from,
                    &relationship.to,
                );
                add_node(
                    graph,
                    seen_nodes,
                    Node {
                        id: relationship_id.clone(),
                        hash: String::new(),
                        kind: "c4-relationship".into(),
                        label: relationship.label.clone().unwrap_or_else(|| {
                            format!("{} -> {}", relationship.from, relationship.to)
                        }),
                        path: Some(format!("{owner_path}#L{}", relationship.line)),
                    },
                );
                add_edge(graph, seen_edges, owner_id, &relationship_id, "contains");
                add_edge(graph, seen_edges, &diagram_id, &relationship_id, "contains");
                add_edge(graph, seen_edges, &relationship_id, from, "from");
                add_edge(graph, seen_edges, &relationship_id, to, "to");
                add_edge(graph, seen_edges, from, to, "relates");
            }
        }
    }
}

pub(crate) fn c4_interface_hash_records(vault: &Vault) -> Vec<C4InterfaceHashRecord> {
    let mut records = Vec::new();
    for note in &vault.notes {
        let owner_id = note_node_id(note.display_id());
        collect_c4_interface_hashes(
            vault,
            &owner_id,
            &note.rel_path,
            &note.c4_diagrams,
            &mut records,
        );
    }
    for artifact in &vault.c4_artifacts {
        let owner_id = c4_artifact_node_id(&artifact.rel_path);
        collect_c4_interface_hashes(
            vault,
            &owner_id,
            &artifact.rel_path,
            &artifact.diagrams,
            &mut records,
        );
    }
    records
}

fn collect_c4_interface_hashes(
    vault: &Vault,
    owner_id: &str,
    owner_path: &str,
    diagrams: &[crate::c4::C4Diagram],
    records: &mut Vec<C4InterfaceHashRecord>,
) {
    for diagram in diagrams {
        for element in &diagram.elements {
            let Some(source) = &element.source else {
                continue;
            };
            let SourceTargetResolution::Resolved { path, .. } = vault.resolve_source_target(source)
            else {
                continue;
            };
            let Some((target, interface_hash)) = interface_anchor_hash(vault, source, &path) else {
                continue;
            };
            let element_id = c4_element_node_id(owner_id, diagram.line, &element.alias);
            records.push(C4InterfaceHashRecord {
                id: c4_interface_node_id(&element_id),
                hash: interface_hash,
                path: owner_path.to_string(),
                line: element.line,
                target,
            });
        }
    }
}

impl State {
    pub(crate) fn c4_interface_hashes(&self) -> BTreeMap<String, String> {
        self.graph
            .nodes
            .iter()
            .filter(|node| node.kind == "c4-interface")
            .map(|node| (node.id.clone(), node.label.clone()))
            .collect()
    }
}

fn interface_anchor_hash(vault: &Vault, source: &str, path: &str) -> Option<(String, String)> {
    let fragment = crate::vault::source_fragment_name(source)?;
    let target = format!("{path}#{fragment}");
    let hash = vault.source_graph().interface_hash(&target)?;
    Some((target, hash))
}

fn add_node(graph: &mut Graph, seen: &mut BTreeSet<String>, node: Node) {
    let mut node = node;
    node.hash = node_hash(&node);
    if seen.insert(node.id.clone()) {
        graph.nodes.push(node);
    }
}

fn add_edge(graph: &mut Graph, seen: &mut BTreeSet<String>, from: &str, to: &str, kind: &str) {
    let key = format!("{from}\0{to}\0{kind}");
    if seen.insert(key) {
        let mut edge = Edge {
            from: from.into(),
            to: to.into(),
            kind: kind.into(),
            hash: String::new(),
        };
        edge.hash = edge_hash(&edge);
        graph.edges.push(edge);
    }
}

fn node_hash(node: &Node) -> String {
    stable_hash(&format!(
        "node\0{}\0{}\0{}\0{}",
        node.id,
        node.kind,
        node.label,
        node.path.as_deref().unwrap_or("")
    ))
}

fn edge_hash(edge: &Edge) -> String {
    stable_hash(&format!("edge\0{}\0{}\0{}", edge.from, edge.kind, edge.to))
}

fn graph_root(graph: &Graph) -> String {
    let mut hashes = graph
        .nodes
        .iter()
        .map(|node| node.hash.as_str())
        .chain(graph.edges.iter().map(|edge| edge.hash.as_str()))
        .collect::<Vec<_>>();
    hashes.sort();
    stable_hash(&hashes.join("\n"))
}

fn stable_hash(value: &str) -> String {
    blake3::hash(value.as_bytes()).to_hex().to_string()
}

fn note_node_id(id: &str) -> String {
    format!("note:{id}")
}

fn code_node_id(path: &str) -> String {
    format!("code:{path}")
}

fn pattern_node_id(id: &str) -> String {
    format!("pattern:{id}")
}

fn import_node_id(path: &str, module: &str) -> String {
    format!("import:{path}:{module}")
}

fn symbol_node_id(id: &str) -> String {
    format!("symbol:{id}")
}

fn external_call_node_id(id: &str) -> String {
    format!("external-call:{id}")
}

fn c4_artifact_node_id(path: &str) -> String {
    format!("c4-artifact:{path}")
}

fn c4_diagram_node_id(owner_id: &str, diagram_line: usize) -> String {
    format!("{owner_id}:c4:{diagram_line}")
}

fn c4_element_node_id(owner_id: &str, diagram_line: usize, alias: &str) -> String {
    format!("{owner_id}:c4:{diagram_line}:{alias}")
}

fn c4_interface_node_id(element_id: &str) -> String {
    format!("{element_id}:interface")
}

fn c4_relationship_node_id(
    owner_id: &str,
    diagram_line: usize,
    relationship_line: usize,
    from: &str,
    to: &str,
) -> String {
    format!("{owner_id}:c4:{diagram_line}:rel:{relationship_line}:{from}:{to}")
}

fn symbol_kind(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Class => "class",
    }
}

fn language_name(language: Language) -> &'static str {
    match language {
        Language::Rust => "rust",
        Language::TypeScript => "typescript",
        Language::JavaScript => "javascript",
        Language::Python => "python",
        Language::Go => "go",
        Language::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::vault::Vault;

    #[test]
    fn disabled_source_index_writes_empty_source_state() {
        let root = unique_temp_dir("criv-disabled-source-state");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]

[index]
source = false
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();

        let vault = Vault::load(&root).unwrap();
        let state = State::build(&root, &vault).unwrap();
        let json = serde_json::to_value(&state).unwrap();

        assert_eq!(json["source-index"].as_array().unwrap().len(), 0);
        assert!(
            json["graph"]["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|node| node["kind"] != "code")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn state_and_snapshot_writes_are_parseable() {
        let root = unique_temp_dir("criv-state-atomic-writes");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();

        let vault = Vault::load(&root).unwrap();
        let (snapshot, _) = write_state(&root, &vault).unwrap();

        let state_path = root.join(".criv/state.json");
        let state: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(state_path).unwrap()).unwrap();
        assert_eq!(state["schema"], STATE_SCHEMA);

        let latest = std::fs::read_to_string(root.join(".criv/latest")).unwrap();
        assert_eq!(latest.trim(), snapshot);

        let snapshot_path = root
            .join(".criv/snapshots")
            .join(format!("{snapshot}.json"));
        let state_contents = std::fs::read_to_string(root.join(".criv/state.json")).unwrap();
        let snapshot_contents = std::fs::read_to_string(snapshot_path).unwrap();
        assert_eq!(state_contents, snapshot_contents);
        assert!(state_contents.ends_with('\n'));
        let snapshot_state: serde_json::Value = serde_json::from_str(&snapshot_contents).unwrap();
        assert_eq!(snapshot_state["schema"], STATE_SCHEMA);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn serialized_state_matches_the_v0_contract_fixture() {
        let root = unique_temp_dir("criv-state-contract-fixture");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]

[patterns."code/entrypoint"]
language = "rust"
pattern = "fn $NAME() { $$$BODY }"
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();

        let vault = Vault::load(&root).unwrap();
        let actual: serde_json::Value =
            serde_json::from_str(&State::build(&root, &vault).unwrap().to_json().unwrap()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/state/criv.state.v0.json")).unwrap();

        assert_eq!(actual, expected);
        assert_eq!(
            State::build(&root, &vault).unwrap().hash().unwrap(),
            "1d5fcf7a117fdfee16082f4fa23b527a639b59926cadcb216ae6e781359c1341"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn state_writes_reject_a_symlinked_criv_directory() {
        use std::os::unix::fs::symlink;

        let root = unique_temp_dir("criv-state-symlink");
        let outside = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();
        symlink(outside.path(), root.join(".criv")).unwrap();

        let error = Vault::load(&root).unwrap_err();

        assert!(error.to_string().contains("symlinked vault path component"));
        assert!(!outside.path().join("state.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn slash_qualified_state_patterns_require_registered_sources() {
        let root = unique_temp_dir("criv-state-slash-patterns");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]

[patterns."tool/no-println"]
language = "rust"
pattern = "println!($$$ARGS)"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn run() {\n    println!(\"blocked\");\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("docs/adr/0001-inline-policy.md"),
            r#"---
id: ADR-0001
kind: decision
title: Inline policy
status: accepted
governs:
  - src/lib.rs
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
---

# Inline policy
"#,
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();

        assert_eq!(
            state_pattern_matches(&root, &vault, "missing/println!($$$ARGS)", &[])
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            state_pattern_matches(&root, &vault, "tool/no-println", &[])
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            state_pattern_matches(&root, &vault, "ADR-0001/no-println", &[])
                .unwrap()
                .len(),
            1
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn c4_diagrams_are_written_to_graph_state() {
        let root = unique_temp_dir("criv-c4-state");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/main.rs"), "fn run() {}\n").unwrap();
        std::fs::write(
            root.join("docs/c4.md"),
            r#"---
id: c4
kind: doc
title: C4
---

# C4

```mermaid
C4Container
System_Boundary(system, "criv") {
    Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
    %% criv:source src/main.rs
    Container(plugin, "Obsidian Plugin", "TypeScript", "Reads generated state")
}
System_Ext(github, "GitHub", "Hosts remote repositories")
Rel(cli, plugin, "writes state for")
```
"#,
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();
        let state = State::build(&root, &vault).unwrap();

        let cli_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-container" && node.label == "criv CLI")
            .expect("c4 container node");
        let plugin_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-container" && node.label == "Obsidian Plugin")
            .expect("second c4 container node");
        let github_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-software-system" && node.label == "GitHub")
            .expect("external software system node");
        assert!(
            github_node
                .path
                .as_deref()
                .is_some_and(|path| path.starts_with("docs/c4.md#L"))
        );
        assert!(
            !state.graph.nodes.iter().any(|node| {
                node.kind.starts_with("c4-")
                    && node.kind != "c4-relationship"
                    && node.label == "criv"
            }),
            "boundary labels must not be emitted as architecture element nodes"
        );
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == "note:c4" && edge.to == cli_node.id && edge.kind == "contains"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == cli_node.id && edge.to == "code:src/main.rs" && edge.kind == "references"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == cli_node.id && edge.to == plugin_node.id && edge.kind == "relates"
        }));
        let relationship_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-relationship" && node.label == "writes state for")
            .expect("labelled c4 relationship node");
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == "note:c4" && edge.to == relationship_node.id && edge.kind == "contains"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == relationship_node.id && edge.to == cli_node.id && edge.kind == "from"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == relationship_node.id && edge.to == plugin_node.id && edge.kind == "to"
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn c4_artifacts_are_written_to_graph_state() {
        let root = unique_temp_dir("criv-c4-artifact-state");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/architecture")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/main.rs"), "fn run() {}\n").unwrap();
        std::fs::write(
            root.join("docs/architecture/02-container.c4"),
            r#"
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
%% criv:source src/main.rs#fn:run
Container(plugin, "Obsidian Plugin", "TypeScript", "Reads generated state")
Rel(cli, plugin, "writes state for")
"#,
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();
        let state = State::build(&root, &vault).unwrap();

        let artifact_node = state
            .graph
            .nodes
            .iter()
            .find(|node| {
                node.kind == "c4-artifact"
                    && node.path.as_deref() == Some("docs/architecture/02-container.c4")
            })
            .expect("c4 artifact node");
        let diagram_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-diagram" && node.label == "container diagram")
            .expect("c4 diagram node");
        let cli_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-container" && node.label == "criv CLI")
            .expect("c4 container node");
        let relationship_node = state
            .graph
            .nodes
            .iter()
            .find(|node| node.kind == "c4-relationship" && node.label == "writes state for")
            .expect("c4 relationship node");

        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == artifact_node.id && edge.to == diagram_node.id && edge.kind == "contains"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == diagram_node.id && edge.to == cli_node.id && edge.kind == "contains"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == diagram_node.id
                && edge.to == relationship_node.id
                && edge.kind == "contains"
        }));
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == cli_node.id && edge.to == "code:src/main.rs" && edge.kind == "references"
        }));
        let interface_node = state
            .graph
            .nodes
            .iter()
            .find(|node| {
                node.kind == "c4-interface"
                    && node.path.as_deref() == Some("src/main.rs#fn:run")
                    && !node.label.is_empty()
            })
            .expect("c4 interface hash node");
        assert!(state.graph.edges.iter().any(|edge| {
            edge.from == cli_node.id
                && edge.to == interface_node.id
                && edge.kind == "tracks-interface"
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    const POLICY_ADR: &str = r#"---
id: ADR-0001
kind: decision
title: No Println
status: accepted
date: 2026-07-25
governs:
  - src/**/*.rs
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
      message: Prefer structured diagnostics.
---

# No Println

## Context

Context.

## Decision

Decision.

## Consequences

Consequences.
"#;

    const PATTERN_ID: &str = "ADR-0001/no-println";

    fn policy_vault(prefix: &str) -> PathBuf {
        let root = unique_temp_dir(prefix);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            "[vault]\ndocs = \"docs\"\nadr = \"adr\"\n\n[source]\nroots = [\"src\"]\n",
        )
        .unwrap();
        std::fs::write(root.join("docs/adr/0001-no-println.md"), POLICY_ADR).unwrap();
        std::fs::write(
            root.join("src/alpha.rs"),
            "fn alpha() {\n    println!(\"alpha\");\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/beta.rs"),
            "fn beta() {\n    println!(\"beta\");\n}\n",
        )
        .unwrap();
        root
    }

    fn matched_files(state: &State) -> Vec<String> {
        state
            .patterns
            .get(PATTERN_ID)
            .map(|matches| matches.iter().map(|matched| matched.file.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn incremental_pattern_matches_reuse_unchanged_files() {
        let root = policy_vault("criv-incremental-pattern-reuse");

        let vault = Vault::load(&root).unwrap();
        let (_, first) = write_state(&root, &vault).unwrap();
        assert_eq!(
            matched_files(&first),
            vec!["src/alpha.rs".to_string(), "src/beta.rs".to_string()],
            "both governed files should match before the edit"
        );
        let alpha_before = first
            .patterns
            .get(PATTERN_ID)
            .unwrap()
            .iter()
            .find(|matched| matched.file == "src/alpha.rs")
            .cloned()
            .expect("alpha match");

        std::fs::write(
            root.join("src/beta.rs"),
            "fn beta() {\n    // moved down\n    println!(\"beta changed\");\n}\n",
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, second) = write_state_incremental(
            &root,
            &vault,
            Some(&first),
            std::slice::from_ref(&"src/beta.rs".to_string()),
        )
        .unwrap();

        let second_matches = second.patterns.get(PATTERN_ID).unwrap();
        let alpha_after = second_matches
            .iter()
            .find(|matched| matched.file == "src/alpha.rs")
            .expect("alpha match survives an unrelated edit");
        assert_eq!(
            &alpha_before, alpha_after,
            "an unchanged file's match must be carried forward byte-identically"
        );

        let beta_after = second_matches
            .iter()
            .find(|matched| matched.file == "src/beta.rs")
            .expect("beta match is rescanned");
        assert_ne!(
            beta_after.range, alpha_before.range,
            "the changed file's match should be rescanned at its new position"
        );
        assert_eq!(matched_files(&second).len(), 2);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_pattern_matches_skip_files_absent_from_the_changed_set() {
        // Pins the reuse contract itself: a file the caller does not report as
        // changed is carried forward from the previous state and is NOT
        // rescanned. Editing alpha on disk while reporting only beta means a
        // full rescan would drop alpha's match, while correct reuse keeps it.
        let root = policy_vault("criv-incremental-pattern-scope");

        let vault = Vault::load(&root).unwrap();
        let (_, first) = write_state(&root, &vault).unwrap();
        assert_eq!(matched_files(&first).len(), 2);

        std::fs::write(root.join("src/alpha.rs"), "fn alpha() {}\n").unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, second) = write_state_incremental(
            &root,
            &vault,
            Some(&first),
            std::slice::from_ref(&"src/beta.rs".to_string()),
        )
        .unwrap();

        assert!(
            matched_files(&second).contains(&"src/alpha.rs".to_string()),
            "a file outside the changed set must be reused, not rescanned"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_rescan_is_empty_when_no_changed_file_is_in_scope() {
        // `README.md` is outside the ADR's `governs:` scope, so the scoped
        // changed set is empty and an empty glob list must keep meaning "nothing
        // in scope". Widening it to "no filter" would rescan the whole vault and
        // pick up the println! added to alpha after the previous state was
        // written — a file the caller never reported as changed.
        let root = policy_vault("criv-incremental-pattern-out-of-scope");
        std::fs::write(root.join("src/alpha.rs"), "fn alpha() {}\n").unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, first) = write_state(&root, &vault).unwrap();
        assert_eq!(matched_files(&first), vec!["src/beta.rs".to_string()]);

        std::fs::write(
            root.join("src/alpha.rs"),
            "fn alpha() {\n    println!(\"alpha\");\n}\n",
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, second) = write_state_incremental(
            &root,
            &vault,
            Some(&first),
            std::slice::from_ref(&"README.md".to_string()),
        )
        .unwrap();

        assert_eq!(
            matched_files(&second),
            vec!["src/beta.rs".to_string()],
            "no governed file changed, so the rescan must contribute nothing"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_pattern_matches_drop_deleted_files() {
        let root = policy_vault("criv-incremental-pattern-delete");

        let vault = Vault::load(&root).unwrap();
        let (_, first) = write_state(&root, &vault).unwrap();
        assert_eq!(matched_files(&first).len(), 2);

        std::fs::remove_file(root.join("src/beta.rs")).unwrap();

        let vault = Vault::load(&root).unwrap();
        let (_, second) = write_state_incremental(
            &root,
            &vault,
            Some(&first),
            std::slice::from_ref(&"src/beta.rs".to_string()),
        )
        .unwrap();

        assert_eq!(
            matched_files(&second),
            vec!["src/alpha.rs".to_string()],
            "a deleted file's match must not survive into the next state"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()))
    }
}
