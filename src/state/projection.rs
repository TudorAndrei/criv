//! The graph rows that one Source file or one note contributes to State.

use std::collections::BTreeSet;

use criv_state_wire::{Edge, EdgeKind, Graph, Node, NodeKind, node_id};

use super::stable_hash::StableHasher;
use crate::markdown::LinkTarget;
use crate::source::{
    EdgeOrigin, Import, ModuleRelationshipRole, Relationship, RelationshipKind, RelationshipTarget,
    SourceFile, Symbol, SymbolId,
};
use crate::vault::{Note, NoteKind, ResolvedLink, SourceTargetResolution, Vault};

/// The facts outside its own input that a projection read, so a later
/// refresh knows when to rebuild it.
#[derive(Debug, Clone, Default)]
pub(super) struct PartitionDependencies {
    pub(super) source_content_paths: BTreeSet<String>,
    pub(super) call_targets: BTreeSet<String>,
    pub(super) defined_symbols: BTreeSet<String>,
    pub(super) catalog_sensitive: bool,
    pub(super) note_catalog_sensitive: bool,
    pub(super) policy_sensitive: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct GraphRows {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

#[derive(Debug, Clone)]
pub(super) struct SourceRows {
    pub(super) code_node: Node,
    pub(super) rows: GraphRows,
    pub(super) dependencies: PartitionDependencies,
}

#[derive(Debug, Clone)]
pub(super) struct NoteRows {
    pub(super) rows: GraphRows,
    pub(super) dependencies: PartitionDependencies,
}

/// A graph whose node ids and edges are unique, in first-insertion order.
#[derive(Default)]
pub(super) struct GraphBuilder {
    graph: Graph,
    seen_nodes: BTreeSet<String>,
    seen_edges: BTreeSet<(String, String, EdgeKind)>,
}

impl GraphBuilder {
    fn node(&mut self, id: String, kind: NodeKind, label: String, path: Option<String>) {
        if !self.seen_nodes.contains(&id) {
            self.push_node(hashed_node(id, kind, label, path));
        }
    }

    pub(super) fn push_node(&mut self, node: Node) {
        if self.seen_nodes.insert(node.id.clone()) {
            self.graph.nodes.push(node);
        }
    }

    fn edge(&mut self, from: &str, to: &str, kind: EdgeKind) {
        if self
            .seen_edges
            .insert((from.to_owned(), to.to_owned(), kind))
        {
            let hash = StableHasher::new("edge")
                .str(from)
                .str(kind.as_str())
                .str(to)
                .finish();
            self.graph.edges.push(Edge {
                from: from.into(),
                to: to.into(),
                kind,
                hash,
            });
        }
    }

    pub(super) fn append_rows(&mut self, rows: &GraphRows) {
        for node in &rows.nodes {
            self.push_node(node.clone());
        }
        for edge in &rows.edges {
            self.edge(&edge.from, &edge.to, edge.kind);
        }
    }

    /// The graph with its root hash.
    pub(super) fn finish(self) -> Graph {
        let mut graph = self.graph;
        graph.root = graph_root(&graph);
        graph
    }

    fn into_rows(self) -> GraphRows {
        GraphRows {
            nodes: self.graph.nodes,
            edges: self.graph.edges,
        }
    }
}

fn hashed_node(id: String, kind: NodeKind, label: String, path: Option<String>) -> Node {
    let hash = StableHasher::new("node")
        .str(&id)
        .str(kind.as_str())
        .str(&label)
        .option_str(path.as_deref())
        .finish();
    Node {
        id,
        hash,
        kind,
        label,
        path,
    }
}

fn graph_root(graph: &Graph) -> String {
    let mut hashes = graph
        .nodes
        .iter()
        .map(|node| node.hash.as_str())
        .chain(graph.edges.iter().map(|edge| edge.hash.as_str()))
        .collect::<Vec<_>>();
    hashes.sort_unstable();
    StableHasher::new("graph").strs(&hashes).finish()
}

fn directive_node_id(path: &str, directive: &Import) -> String {
    node_id::directive(
        path,
        directive.kind.as_str(),
        &directive.module,
        directive.site,
    )
}

fn symbol_label(symbol: &Symbol) -> String {
    symbol.arity.map_or_else(
        || symbol.name.clone(),
        |arity| format!("{}/{arity}", symbol.name),
    )
}

pub(super) fn source_rows(vault: &Vault, file: &SourceFile) -> SourceRows {
    let mut graph = GraphBuilder::default();
    let mut dependencies = PartitionDependencies::default();
    let file_id = node_id::code(&file.path);

    for import in &file.imports {
        let import_id = directive_node_id(&file.path, import);
        graph.node(
            import_id.clone(),
            import.kind.into(),
            import.module.clone(),
            Some(node_id::line_location(&file.path, import.line)),
        );
        graph.edge(&file_id, &import_id, EdgeKind::Imports);
    }

    for symbol in &file.symbols {
        dependencies.defined_symbols.insert(symbol.name.clone());
        let symbol_id = node_id::symbol(&symbol.id.display());
        graph.node(
            symbol_id.clone(),
            symbol.kind.into(),
            symbol_label(symbol),
            Some(node_id::range_location(
                &symbol.id.path,
                symbol.range.start_line,
                symbol.range.end_line,
            )),
        );
        graph.edge(&file_id, &symbol_id, EdgeKind::Contains);
        if let Some(parent_id) = &symbol.parent_id {
            graph.edge(
                &node_id::symbol(&parent_id.display()),
                &symbol_id,
                EdgeKind::Contains,
            );
        }
        for edge in vault.source_graph().resolved_edges(symbol) {
            match edge.origin {
                EdgeOrigin::Call(call) => {
                    dependencies.call_targets.insert(call.target.clone());
                    let target = if let Some(target) = edge.target {
                        node_id::symbol(&target.display())
                    } else {
                        dependencies.catalog_sensitive = true;
                        let target = node_id::external_call(&call.target);
                        graph.node(
                            target.clone(),
                            NodeKind::ExternalCall,
                            call.target.clone(),
                            Some(node_id::line_location(&symbol.id.path, call.line)),
                        );
                        target
                    };
                    graph.edge(&symbol_id, &target, EdgeKind::Calls);
                }
                EdgeOrigin::Relationship(relationship) => project_relationship(
                    vault,
                    &mut graph,
                    &mut dependencies,
                    symbol,
                    relationship,
                    edge.target.as_ref(),
                ),
            }
        }
    }

    SourceRows {
        code_node: hashed_node(
            file_id,
            NodeKind::Code,
            format!("{} ({})", file.path, file.language.as_str()),
            Some(file.path.clone()),
        ),
        rows: graph.into_rows(),
        dependencies,
    }
}

fn project_relationship(
    vault: &Vault,
    graph: &mut GraphBuilder,
    dependencies: &mut PartitionDependencies,
    symbol: &Symbol,
    relationship: &Relationship,
    resolved: Option<&SymbolId>,
) {
    let source_graph = vault.source_graph();
    let label = match &relationship.target {
        RelationshipTarget::Dynamic { label, .. } => label.clone(),
        _ => source_graph.relationship_target_label(&symbol.id, relationship),
    };
    if let Some(target) = resolved.and_then(|target| source_graph.symbol_name(target)) {
        dependencies.call_targets.insert(target.to_string());
    } else if let RelationshipTarget::Callable { name, .. } = &relationship.target {
        dependencies.call_targets.insert(name.clone());
    } else if let RelationshipTarget::Module { module, .. } = &relationship.target {
        dependencies.call_targets.insert(module.clone());
    }
    if resolved.is_none() && !matches!(relationship.target, RelationshipTarget::Dynamic { .. }) {
        dependencies.catalog_sensitive = true;
    }
    let target = resolved.map_or_else(
        || relationship_target_node_id(relationship, &label),
        |target| node_id::symbol(&target.display()),
    );
    if resolved.is_none() {
        graph.node(
            target.clone(),
            relationship_target_node_kind(relationship),
            label,
            Some(node_id::line_location(&symbol.id.path, relationship.line)),
        );
    }
    graph.edge(
        &node_id::symbol(&symbol.id.display()),
        &target,
        relationship_edge_kind(relationship),
    );
}

fn relationship_target_node_id(relationship: &Relationship, label: &str) -> String {
    match &relationship.target {
        RelationshipTarget::Dynamic { id, .. } => node_id::dynamic_call(id),
        RelationshipTarget::Callable { .. } => node_id::external_call(label),
        RelationshipTarget::Module { .. } => node_id::external_module(label),
    }
}

const fn relationship_target_node_kind(relationship: &Relationship) -> NodeKind {
    match &relationship.target {
        RelationshipTarget::Dynamic { .. } => NodeKind::DynamicCall,
        RelationshipTarget::Callable { .. } => NodeKind::ExternalCall,
        RelationshipTarget::Module { .. } => NodeKind::ExternalModule,
    }
}

const fn relationship_edge_kind(relationship: &Relationship) -> EdgeKind {
    match (&relationship.kind, &relationship.target) {
        (RelationshipKind::Call, _) => EdgeKind::Calls,
        (RelationshipKind::Capture, _) => EdgeKind::Captures,
        (RelationshipKind::Delegate, _) => EdgeKind::Delegates,
        (
            RelationshipKind::ProtocolImplementation,
            RelationshipTarget::Module {
                role: ModuleRelationshipRole::Protocol,
                ..
            },
        ) => EdgeKind::ImplementsProtocol,
        (
            RelationshipKind::ProtocolImplementation,
            RelationshipTarget::Module {
                role: ModuleRelationshipRole::ForType,
                ..
            },
        ) => EdgeKind::ImplementsFor,
        (RelationshipKind::BehaviourImplementation, _) => EdgeKind::ImplementsBehaviour,
        (RelationshipKind::ProtocolImplementation, _) => EdgeKind::ProtocolImplementation,
    }
}

pub(super) fn note_rows(vault: &Vault, note: &Note) -> NoteRows {
    let mut graph = GraphBuilder::default();
    let mut dependencies = PartitionDependencies {
        note_catalog_sensitive: !note.wiki_links().is_empty(),
        policy_sensitive: note.kind() == NoteKind::Decision,
        ..PartitionDependencies::default()
    };
    let kind = match note.kind() {
        NoteKind::Decision => NodeKind::Decision,
        NoteKind::Doc | NoteKind::Unknown => NodeKind::Doc,
    };
    let note_id = node_id::note(note.display_id());
    graph.node(
        note_id.clone(),
        kind,
        note.title()
            .unwrap_or_else(|| note.display_id())
            .to_string(),
        Some(note.rel_path().to_string()),
    );

    for target in note.targets_symbols() {
        dependencies.catalog_sensitive = true;
        match vault.resolve_source_target(LinkTarget::parse(target)) {
            SourceTargetResolution::Resolved { path, .. } => {
                dependencies.source_content_paths.insert(path.clone());
                graph.edge(&note_id, &node_id::code(&path), EdgeKind::References);
            }
            SourceTargetResolution::MissingFragment { path } => {
                dependencies.source_content_paths.insert(path);
            }
            SourceTargetResolution::MissingFile => {}
        }
    }

    for heading in note.headings() {
        let heading_id = node_id::note_heading(&note_id, &crate::identity::slug(&heading.text));
        graph.node(
            heading_id.clone(),
            NodeKind::DocHeading,
            heading.text.clone(),
            Some(node_id::heading_location(
                note.rel_path(),
                heading.line,
                heading.level,
            )),
        );
        graph.edge(&note_id, &heading_id, EdgeKind::Contains);
    }

    if !note.effective_governs().is_empty() {
        dependencies.catalog_sensitive = true;
    }
    for source_file in vault.governed_files(note) {
        graph.edge(&note_id, &node_id::code(&source_file), EdgeKind::Governs);
    }

    for superseded in note.supersedes() {
        graph.edge(&note_id, &node_id::note(superseded), EdgeKind::Supersedes);
    }

    for link in note.wiki_links() {
        match vault.resolve_link(link.target()) {
            ResolvedLink::Note { id, .. } => {
                graph.edge(&note_id, &node_id::note(&id), EdgeKind::Cites);
            }
            ResolvedLink::Source { path, .. } => {
                dependencies.catalog_sensitive = true;
                if link.target().symbol().is_some() {
                    dependencies.source_content_paths.insert(path.clone());
                }
                graph.edge(&note_id, &node_id::code(&path), EdgeKind::References);
            }
            ResolvedLink::Pattern { id } => {
                dependencies.policy_sensitive = true;
                let pattern_id = node_id::pattern(&id);
                graph.node(pattern_id.clone(), NodeKind::Pattern, id, None);
                graph.edge(&note_id, &pattern_id, EdgeKind::References);
            }
            ResolvedLink::Broken => {
                dependencies.catalog_sensitive = true;
                if let SourceTargetResolution::MissingFragment { path } =
                    vault.resolve_source_target(link.target())
                {
                    dependencies.source_content_paths.insert(path);
                }
            }
        }
    }

    NoteRows {
        rows: graph.into_rows(),
        dependencies,
    }
}
