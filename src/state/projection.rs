use super::{
    C4Artifact, DirectiveKind, EdgeKind, Graph, GraphBuilder, ModuleRelationshipRole, NodeKind,
    Note, NoteKind, PartitionDependencies, PartitionKey, Relationship, RelationshipKind,
    RelationshipTarget, ResolvedLink, RowPartition, SourceFile, SourcePartition,
    SourceTargetResolution, Symbol, Vault, c4_artifact_input_fingerprint, c4_artifact_node_id,
    code_node_id, directive_node_id, external_call_node_id, external_module_node_id, graph_root,
    graph_rows, hashed_node, interface_anchor_hash, likec4_element_node_id,
    likec4_interface_node_id, model_array, note_input_fingerprint, note_node_id, partition_meta,
    pattern_node_id, relationship_endpoint, source_input_fingerprint, symbol_label, symbol_node_id,
};
use crate::source::SymbolKind;

impl From<DirectiveKind> for NodeKind {
    fn from(kind: DirectiveKind) -> Self {
        match kind {
            DirectiveKind::Alias => Self::Alias,
            DirectiveKind::Import | DirectiveKind::Legacy => Self::Import,
            DirectiveKind::Require => Self::Require,
            DirectiveKind::Use => Self::Use,
        }
    }
}

impl From<SymbolKind> for NodeKind {
    fn from(kind: SymbolKind) -> Self {
        match kind {
            SymbolKind::Function => Self::Function,
            SymbolKind::Method => Self::Method,
            SymbolKind::Class => Self::Class,
            SymbolKind::Module => Self::Module,
            SymbolKind::Protocol => Self::Protocol,
            SymbolKind::Implementation => Self::Implementation,
            SymbolKind::Struct => Self::Struct,
            SymbolKind::Exception => Self::Exception,
            SymbolKind::Behaviour => Self::Behaviour,
            SymbolKind::Macro => Self::Macro,
            SymbolKind::Guard => Self::Guard,
            SymbolKind::Callback => Self::Callback,
            SymbolKind::MacroCallback => Self::MacroCallback,
        }
    }
}

pub(super) fn build_source_partition(vault: &Vault, file: &SourceFile) -> SourcePartition {
    let mut graph = GraphBuilder::default();
    let mut dependencies = PartitionDependencies::default();
    let file_id = code_node_id(&file.path);

    for import in &file.imports {
        let import_id = directive_node_id(file, import);
        graph.node(
            import_id.clone(),
            import.kind.into(),
            import.module.clone(),
            Some(format!("{}#L{}", file.path, import.line)),
        );
        graph.edge(&file_id, &import_id, EdgeKind::Imports);
    }

    for symbol in &file.symbols {
        dependencies.defined_symbols.insert(symbol.name.clone());
        let symbol_id = symbol_node_id(&symbol.id.display());
        graph.node(
            symbol_id.clone(),
            symbol.kind.into(),
            symbol_label(symbol),
            Some(format!(
                "{}#L{}-L{}",
                symbol.id.path, symbol.range.start_line, symbol.range.end_line
            )),
        );
        graph.edge(&file_id, &symbol_id, EdgeKind::Contains);
        if let Some(parent) = &symbol.parent
            && let Some(parent_id) = vault
                .source_graph()
                .resolve_symbol(&format!("{}#{}", symbol.id.path, parent))
        {
            graph.edge(
                &symbol_node_id(&parent_id.display()),
                &symbol_id,
                EdgeKind::Contains,
            );
        }
        if let Some(owner) = &symbol.owner
            && let Some(parent) = file.symbols.iter().find(|candidate| {
                candidate.id != symbol.id
                    && candidate.arity.is_none()
                    && candidate.owner.as_ref() == Some(owner)
            })
        {
            graph.edge(
                &symbol_node_id(&parent.id.display()),
                &symbol_id,
                EdgeKind::Contains,
            );
        }
        for call in &symbol.calls {
            dependencies.call_targets.insert(call.target.clone());
            let resolved = vault.source_graph().resolve_call(&symbol.id, &call.target);
            if resolved.is_none() {
                dependencies.catalog_sensitive = true;
            }
            let target = resolved.map_or_else(
                || external_call_node_id(&call.target),
                |target| symbol_node_id(&target.display()),
            );
            if target.starts_with("external-call:") {
                graph.node(
                    target.clone(),
                    NodeKind::ExternalCall,
                    call.target.clone(),
                    Some(format!("{}#L{}", symbol.id.path, call.line)),
                );
            }
            graph.edge(&symbol_id, &target, EdgeKind::Calls);
        }
        for relationship in &symbol.relationships {
            project_relationship(vault, &mut graph, &mut dependencies, symbol, relationship);
        }
    }

    SourcePartition {
        meta: partition_meta(
            PartitionKey::Source(file.path.clone()),
            source_input_fingerprint(file),
            dependencies,
        ),
        code_node: hashed_node(
            file_id,
            NodeKind::Code,
            format!("{} ({})", file.path, file.language.as_str()),
            Some(file.path.clone()),
        ),
        rows: graph_rows(graph.finish()),
    }
}

fn project_relationship(
    vault: &Vault,
    graph: &mut GraphBuilder,
    dependencies: &mut PartitionDependencies,
    symbol: &Symbol,
    relationship: &Relationship,
) {
    let source_graph = vault.source_graph();
    let label = match &relationship.target {
        RelationshipTarget::Dynamic { label, .. } => label.clone(),
        _ => source_graph.relationship_target_label(&symbol.id, relationship),
    };
    let resolved = source_graph.resolve_relationship(&symbol.id, relationship);
    if let Some(target) = resolved
        .as_ref()
        .and_then(|target| source_graph.symbol_name(target))
    {
        dependencies.call_targets.insert(target.to_string());
    } else if let RelationshipTarget::Callable { name, .. } = &relationship.target {
        dependencies.call_targets.insert(name.clone());
    } else if let RelationshipTarget::Module { module, .. } = &relationship.target {
        dependencies.call_targets.insert(module.clone());
    }
    if resolved.is_none() && !matches!(relationship.target, RelationshipTarget::Dynamic { .. }) {
        dependencies.catalog_sensitive = true;
    }
    let target = resolved.as_ref().map_or_else(
        || relationship_target_node_id(relationship, &label),
        |target| symbol_node_id(&target.display()),
    );
    if resolved.is_none() {
        graph.node(
            target.clone(),
            relationship_target_node_kind(relationship),
            label,
            Some(format!("{}#L{}", symbol.id.path, relationship.line)),
        );
    }
    graph.edge(
        &symbol_node_id(&symbol.id.display()),
        &target,
        relationship_edge_kind(relationship),
    );
}

fn relationship_target_node_id(relationship: &Relationship, label: &str) -> String {
    match &relationship.target {
        RelationshipTarget::Dynamic { id, .. } => format!("dynamic-call:{id}"),
        RelationshipTarget::Callable { .. } => external_call_node_id(label),
        RelationshipTarget::Module { .. } => external_module_node_id(label),
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

pub(super) fn build_note_partition(vault: &Vault, note: &Note) -> RowPartition {
    let mut graph = GraphBuilder::default();
    let mut dependencies = PartitionDependencies {
        note_catalog_sensitive: !note.wiki_links.is_empty(),
        policy_sensitive: note.kind == NoteKind::Decision,
        ..PartitionDependencies::default()
    };
    let kind = match note.kind {
        NoteKind::Decision => NodeKind::Decision,
        NoteKind::Doc | NoteKind::Unknown => NodeKind::Doc,
    };
    let note_id = note_node_id(note.display_id());
    graph.node(
        note_id.clone(),
        kind,
        note.title
            .clone()
            .unwrap_or_else(|| note.display_id().to_string()),
        Some(note.rel_path.clone()),
    );

    for target in &note.targets_symbols {
        dependencies.catalog_sensitive = true;
        match vault.resolve_source_target(target) {
            SourceTargetResolution::Resolved { path, .. } => {
                dependencies.source_content_paths.insert(path.clone());
                graph.edge(&note_id, &code_node_id(&path), EdgeKind::References);
            }
            SourceTargetResolution::MissingFragment { path } => {
                dependencies.source_content_paths.insert(path);
            }
            SourceTargetResolution::MissingFile => {}
        }
    }

    for heading in &note.headings {
        let heading_id = format!("{note_id}#{}", crate::identity::kebab(&heading.text));
        graph.node(
            heading_id.clone(),
            NodeKind::DocHeading,
            heading.text.clone(),
            Some(format!(
                "{}#L{}:H{}",
                note.rel_path, heading.line, heading.level
            )),
        );
        graph.edge(&note_id, &heading_id, EdgeKind::Contains);
    }

    let governs = Vault::effective_governs(note);
    if !governs.is_empty() {
        dependencies.catalog_sensitive = true;
    }
    for source_file in vault.source_files_matching_globs(&governs) {
        graph.edge(&note_id, &code_node_id(&source_file), EdgeKind::Governs);
    }

    for superseded in &note.supersedes {
        graph.edge(&note_id, &note_node_id(superseded), EdgeKind::Supersedes);
    }

    for link in &note.wiki_links {
        match vault.resolve_link(&link.target) {
            ResolvedLink::Note { id } => {
                graph.edge(&note_id, &note_node_id(&id), EdgeKind::Cites);
            }
            ResolvedLink::Source { path, .. } => {
                dependencies.catalog_sensitive = true;
                if crate::vault::source_fragment_name(&link.target).is_some() {
                    dependencies.source_content_paths.insert(path.clone());
                }
                graph.edge(&note_id, &code_node_id(&path), EdgeKind::References);
            }
            ResolvedLink::Pattern { id } => {
                dependencies.policy_sensitive = true;
                let pattern_id = pattern_node_id(&id);
                graph.node(pattern_id.clone(), NodeKind::Pattern, id, None);
                graph.edge(&note_id, &pattern_id, EdgeKind::References);
            }
            ResolvedLink::Broken => {
                dependencies.catalog_sensitive = true;
                if let SourceTargetResolution::MissingFragment { path } =
                    vault.resolve_source_target(&link.target)
                {
                    dependencies.source_content_paths.insert(path);
                }
            }
        }
    }

    collect_graph_source_dependencies(graph.graph(), &mut dependencies);

    RowPartition {
        meta: partition_meta(
            PartitionKey::Note(note.rel_path.clone()),
            note_input_fingerprint(note),
            dependencies,
        ),
        rows: graph_rows(graph.finish()),
    }
}

pub(super) fn build_c4_artifact_partition(artifact: &C4Artifact) -> RowPartition {
    let mut graph = GraphBuilder::default();
    graph.node(
        c4_artifact_node_id(&artifact.rel_path),
        NodeKind::ArchitectureSource,
        artifact.rel_path.clone(),
        Some(artifact.rel_path.clone()),
    );
    RowPartition {
        meta: partition_meta(
            PartitionKey::C4Artifact(artifact.rel_path.clone()),
            c4_artifact_input_fingerprint(artifact),
            PartitionDependencies::default(),
        ),
        rows: graph_rows(graph.finish()),
    }
}

fn collect_graph_source_dependencies(graph: &Graph, dependencies: &mut PartitionDependencies) {
    for node in &graph.nodes {
        if node.kind == NodeKind::ArchitectureInterface
            && let Some(path) = node
                .path
                .as_deref()
                .and_then(|target| target.split('#').next())
        {
            dependencies.source_content_paths.insert(path.to_string());
        }
    }
}

pub(super) fn with_likec4_model(graph: Graph, vault: &Vault, model: &serde_json::Value) -> Graph {
    let mut graph = GraphBuilder::extending(graph);
    let workspace_id = "architecture:likec4";
    graph.node(
        workspace_id.into(),
        NodeKind::ArchitectureWorkspace,
        "LikeC4 architecture".into(),
        Some(vault.likec4_workspace.path.clone()),
    );

    for element in model_array(model, "elements") {
        let Some(id) = element.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let element_id = likec4_element_node_id(id);
        graph.node(
            element_id.clone(),
            NodeKind::ArchitectureElement,
            element
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(id)
                .to_string(),
            Some(vault.likec4_workspace.path.clone()),
        );
        graph.edge(workspace_id, &element_id, EdgeKind::Contains);
    }

    for relationship in model_array(model, "relationships") {
        let Some(source) = relationship_endpoint(relationship, "source") else {
            continue;
        };
        let Some(target) = relationship_endpoint(relationship, "target") else {
            continue;
        };
        graph.edge(
            &likec4_element_node_id(source),
            &likec4_element_node_id(target),
            EdgeKind::Relates,
        );
    }

    for link in model_array(model, "sourceLinks") {
        let Some(element) = link.get("element").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(target) = link.get("target").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let SourceTargetResolution::Resolved { path, .. } = vault.resolve_source_target(target)
        else {
            continue;
        };
        let element_id = likec4_element_node_id(element);
        graph.edge(&element_id, &code_node_id(&path), EdgeKind::References);
        if let Some((target, interface_hash)) = interface_anchor_hash(vault, target, &path) {
            let interface_id = likec4_interface_node_id(element);
            graph.node(
                interface_id.clone(),
                NodeKind::ArchitectureInterface,
                interface_hash,
                Some(target),
            );
            graph.edge(&element_id, &interface_id, EdgeKind::TracksInterface);
        }
    }

    let mut graph = graph.finish();
    graph.nodes.sort_by(|left, right| left.id.cmp(&right.id));
    graph.edges.sort_by(|left, right| {
        (&left.from, &left.to, &left.kind).cmp(&(&right.from, &right.to, &right.kind))
    });
    graph.root = graph_root(&graph);
    graph
}
