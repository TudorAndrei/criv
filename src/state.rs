use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Serialize;

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
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Node {
    id: String,
    kind: String,
    label: String,
    path: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Edge {
    from: String,
    to: String,
    kind: String,
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
    frecency: u32,
}

impl State {
    pub(crate) fn build(vault: &Vault) -> Self {
        let mut graph = Graph::default();
        let mut seen_nodes = BTreeSet::new();
        let mut seen_edges = BTreeSet::new();

        for source_file in vault.source_files() {
            add_node(
                &mut graph,
                &mut seen_nodes,
                Node {
                    id: code_node_id(source_file),
                    kind: "code".into(),
                    label: source_file.clone(),
                    path: Some(source_file.clone()),
                },
            );
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
                frecency: (vault.source_files().len() - index) as u32,
            })
            .collect::<Vec<_>>();
        source_index.sort_by(|left, right| left.path.cmp(&right.path));

        let patterns = vault
            .patterns()
            .iter()
            .map(|pattern| (pattern.clone(), Vec::new()))
            .collect::<BTreeMap<_, _>>();

        Self {
            schema: STATE_SCHEMA,
            graph,
            registered_patterns: vault.patterns().iter().cloned().collect(),
            patterns,
            source_index,
        }
    }

    pub(crate) fn write(&self, root: &Path) -> Result<()> {
        let path = root.join(".criv/state.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = serde_json::to_string_pretty(self)
            .map_err(|err| CrivError::new(format!("failed to serialize state: {err}")))?;
        fs::write(path, format!("{contents}\n"))?;
        Ok(())
    }
}

pub(crate) fn write_state(root: &Path, vault: &Vault) -> Result<()> {
    State::build(vault).write(root)
}

fn add_node(graph: &mut Graph, seen: &mut BTreeSet<String>, node: Node) {
    if seen.insert(node.id.clone()) {
        graph.nodes.push(node);
    }
}

fn add_edge(graph: &mut Graph, seen: &mut BTreeSet<String>, from: &str, to: &str, kind: &str) {
    let key = format!("{from}\0{to}\0{kind}");
    if seen.insert(key) {
        graph.edges.push(Edge {
            from: from.into(),
            to: to.into(),
            kind: kind.into(),
        });
    }
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
