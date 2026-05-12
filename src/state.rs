use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::source_graph::{Language, SymbolKind};
use crate::structural;
use crate::vault::{NoteKind, ResolvedLink, Vault, source_fragment_path};
use crate::{CrivError, Result};

pub(crate) const STATE_SCHEMA: &str = "criv.state.v0";

#[derive(Debug, Serialize)]
pub(crate) struct State {
    schema: &'static str,
    graph: Graph,
    #[serde(rename = "registered-patterns")]
    registered_patterns: Vec<String>,
    patterns: BTreeMap<String, Vec<PatternMatch>>,
    #[serde(rename = "source-index")]
    source_index: Vec<SourceIndexEntry>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct Graph {
    root: String,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Node {
    id: String,
    hash: String,
    kind: String,
    label: String,
    path: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Edge {
    from: String,
    to: String,
    kind: String,
    hash: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PatternMatch {
    file: String,
    range: Option<String>,
    captures: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SourceIndexEntry {
    path: String,
    mime: Option<String>,
    frecency: u32,
}

impl State {
    pub(crate) fn build(root: &Path, vault: &Vault) -> Result<Self> {
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
                if let Some(parent) = &symbol.parent {
                    if let Some(parent_id) = vault
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
                if let Some((path, _)) = vault.resolve_source_path(source_fragment_path(target)) {
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

            for governs in vault.effective_governs(note) {
                for source_file in vault.source_files_matching_glob(&governs) {
                    add_edge(
                        &mut graph,
                        &mut seen_edges,
                        &note_id,
                        &code_node_id(&source_file),
                        "governs",
                    );
                }
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
        }

        let mut source_index = vault
            .source_files()
            .iter()
            .enumerate()
            .map(|(index, path)| SourceIndexEntry {
                path: path.clone(),
                mime: mime_guess::from_path(path).first_raw().map(str::to_string),
                frecency: (vault.source_files().len() - index) as u32,
            })
            .collect::<Vec<_>>();
        source_index.sort_by(|left, right| left.path.cmp(&right.path));

        let mut patterns = BTreeMap::new();
        for pattern_id in vault.patterns() {
            patterns.insert(
                pattern_id.clone(),
                state_pattern_matches(root, vault, pattern_id)?,
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

    pub(crate) fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|err| CrivError::new(format!("failed to serialize state: {err}")))
    }

    pub(crate) fn hash(&self) -> Result<String> {
        let contents = self.to_json()?;
        Ok(stable_hash(&contents))
    }

    pub(crate) fn write(&self, root: &Path) -> Result<()> {
        let path = root.join(".criv/state.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = self.to_json()?;
        fs::write(path, format!("{contents}\n"))?;
        Ok(())
    }

    pub(crate) fn write_snapshot(&self, root: &Path) -> Result<String> {
        let hash = self.hash()?;
        let criv_dir = root.join(".criv");
        let snapshots = criv_dir.join("snapshots");
        fs::create_dir_all(&criv_dir)?;
        fs::create_dir_all(&snapshots)?;
        let path = snapshots.join(format!("{hash}.json"));
        if !path.exists() {
            fs::write(path, format!("{}\n", self.to_json()?))?;
        }
        fs::write(criv_dir.join("latest"), format!("{hash}\n"))?;
        Ok(hash)
    }
}

pub(crate) fn write_state(root: &Path, vault: &Vault) -> Result<String> {
    let state = State::build(root, vault)?;
    state.write(root)?;
    state.write_snapshot(root)
}

fn state_pattern_matches(
    root: &Path,
    vault: &Vault,
    pattern_id: &str,
) -> Result<Vec<PatternMatch>> {
    let matches = if let Some((adr_id, local_id)) = pattern_id.split_once('/') {
        let pattern = local_id;
        let scopes = vault
            .resolve_note(adr_id)
            .map(|note| vault.effective_governs(note))
            .unwrap_or_else(|| vec!["**".into()]);
        structural::find_policy_pattern(root, vault, pattern_id, pattern, &scopes)?
    } else if vault.config.pattern_defs.contains_key(pattern_id) {
        structural::find_pattern_id(root, vault, pattern_id, &[])?
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

pub(crate) fn note_node_id(id: &str) -> String {
    format!("note:{id}")
}

pub(crate) fn code_node_id(path: &str) -> String {
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
