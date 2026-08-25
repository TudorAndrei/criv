use super::{
    BTreeSet, C4Artifact, Graph, ModuleRelationshipRole, Node, Note, NoteKind,
    PartitionDependencies, PartitionKey, Relationship, RelationshipKind, RelationshipTarget,
    ResolvedLink, RowPartition, SourceFile, SourcePartition, SourceTargetResolution, Symbol, Vault,
    add_edge, add_node, c4_artifact_input_fingerprint, c4_artifact_node_id, code_node_id,
    directive_node_id, external_call_node_id, external_module_node_id, graph_root, graph_rows,
    interface_anchor_hash, likec4_element_node_id, likec4_interface_node_id, model_array,
    note_input_fingerprint, note_node_id, partition_meta, pattern_node_id, relationship_endpoint,
    source_input_fingerprint, symbol_label, symbol_node_id,
};

#[expect(
    clippy::too_many_lines,
    reason = "source partition building preserves graph insertion order and dependencies together"
)]
pub(super) fn build_source_partition(vault: &Vault, file: &SourceFile) -> SourcePartition {
    let mut graph = Graph::default();
    let mut seen_nodes = BTreeSet::new();
    let mut seen_edges = BTreeSet::new();
    let mut dependencies = PartitionDependencies::default();
    let file_id = code_node_id(&file.path);

    for import in &file.imports {
        let import_id = directive_node_id(file, import);
        add_node(
            &mut graph,
            &mut seen_nodes,
            Node {
                id: import_id.clone(),
                hash: String::new(),
                kind: import.kind.as_str().into(),
                label: import.module.clone(),
                path: Some(format!("{}#L{}", file.path, import.line)),
            },
        );
        add_edge(&mut graph, &mut seen_edges, &file_id, &import_id, "imports");
    }

    for symbol in &file.symbols {
        dependencies.defined_symbols.insert(symbol.name.clone());
        let symbol_id = symbol_node_id(&symbol.id.display());
        add_node(
            &mut graph,
            &mut seen_nodes,
            Node {
                id: symbol_id.clone(),
                hash: String::new(),
                kind: symbol.kind.as_str().into(),
                label: symbol_label(symbol),
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
        if let Some(owner) = &symbol.owner
            && let Some(parent) = file.symbols.iter().find(|candidate| {
                candidate.id != symbol.id
                    && candidate.arity.is_none()
                    && candidate.owner.as_ref() == Some(owner)
            })
        {
            add_edge(
                &mut graph,
                &mut seen_edges,
                &symbol_node_id(&parent.id.display()),
                &symbol_id,
                "contains",
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
        for relationship in &symbol.relationships {
            project_relationship(
                vault,
                &mut graph,
                &mut seen_nodes,
                &mut seen_edges,
                &mut dependencies,
                symbol,
                relationship,
            );
        }
    }

    SourcePartition {
        meta: partition_meta(
            PartitionKey::Source(file.path.clone()),
            source_input_fingerprint(file),
            dependencies,
        ),
        code_node: Node {
            id: file_id,
            hash: String::new(),
            kind: "code".into(),
            label: format!("{} ({})", file.path, file.language.as_str()),
            path: Some(file.path.clone()),
        },
        rows: graph_rows(graph),
    }
}

fn project_relationship(
    vault: &Vault,
    graph: &mut Graph,
    seen_nodes: &mut BTreeSet<String>,
    seen_edges: &mut BTreeSet<String>,
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
        add_node(
            graph,
            seen_nodes,
            Node {
                id: target.clone(),
                hash: String::new(),
                kind: relationship_target_node_kind(relationship).into(),
                label,
                path: Some(format!("{}#L{}", symbol.id.path, relationship.line)),
            },
        );
    }
    add_edge(
        graph,
        seen_edges,
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

const fn relationship_target_node_kind(relationship: &Relationship) -> &'static str {
    match &relationship.target {
        RelationshipTarget::Dynamic { .. } => "dynamic-call",
        RelationshipTarget::Callable { .. } => "external-call",
        RelationshipTarget::Module { .. } => "external-module",
    }
}

const fn relationship_edge_kind(relationship: &Relationship) -> &'static str {
    match (&relationship.kind, &relationship.target) {
        (RelationshipKind::Call, _) => "calls",
        (RelationshipKind::Capture, _) => "captures",
        (RelationshipKind::Delegate, _) => "delegates",
        (
            RelationshipKind::ProtocolImplementation,
            RelationshipTarget::Module {
                role: ModuleRelationshipRole::Protocol,
                ..
            },
        ) => "implements-protocol",
        (
            RelationshipKind::ProtocolImplementation,
            RelationshipTarget::Module {
                role: ModuleRelationshipRole::ForType,
                ..
            },
        ) => "implements-for",
        (RelationshipKind::BehaviourImplementation, _) => "implements-behaviour",
        _ => relationship.kind.as_str(),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "note partition building keeps graph and dependency construction synchronized"
)]
pub(super) fn build_note_partition(vault: &Vault, note: &Note) -> RowPartition {
    let mut graph = Graph::default();
    let mut seen_nodes = BTreeSet::new();
    let mut seen_edges = BTreeSet::new();
    let mut dependencies = PartitionDependencies {
        note_catalog_sensitive: !note.wiki_links.is_empty(),
        policy_sensitive: note.kind == NoteKind::Decision,
        ..PartitionDependencies::default()
    };
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
        dependencies.catalog_sensitive = true;
        match vault.resolve_source_target(target) {
            SourceTargetResolution::Resolved { path, .. } => {
                dependencies.source_content_paths.insert(path.clone());
                add_edge(
                    &mut graph,
                    &mut seen_edges,
                    &note_id,
                    &code_node_id(&path),
                    "references",
                );
            }
            SourceTargetResolution::MissingFragment { path } => {
                dependencies.source_content_paths.insert(path);
            }
            SourceTargetResolution::MissingFile => {}
        }
    }

    for heading in &note.headings {
        let heading_id = format!("{note_id}#{}", crate::identity::kebab(&heading.text));
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

    let governs = Vault::effective_governs(note);
    if !governs.is_empty() {
        dependencies.catalog_sensitive = true;
    }
    for source_file in vault.source_files_matching_globs(&governs) {
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
                dependencies.catalog_sensitive = true;
                if crate::vault::source_fragment_name(&link.target).is_some() {
                    dependencies.source_content_paths.insert(path.clone());
                }
                add_edge(
                    &mut graph,
                    &mut seen_edges,
                    &note_id,
                    &code_node_id(&path),
                    "references",
                );
            }
            ResolvedLink::Pattern { id } => {
                dependencies.policy_sensitive = true;
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

    collect_graph_source_dependencies(&graph, &mut dependencies);

    RowPartition {
        meta: partition_meta(
            PartitionKey::Note(note.rel_path.clone()),
            note_input_fingerprint(note),
            dependencies,
        ),
        rows: graph_rows(graph),
    }
}

pub(super) fn build_c4_artifact_partition(artifact: &C4Artifact) -> RowPartition {
    let mut graph = Graph::default();
    let mut seen_nodes = BTreeSet::new();
    let artifact_id = c4_artifact_node_id(&artifact.rel_path);
    add_node(
        &mut graph,
        &mut seen_nodes,
        Node {
            id: artifact_id,
            hash: String::new(),
            kind: "architecture-source".into(),
            label: artifact.rel_path.clone(),
            path: Some(artifact.rel_path.clone()),
        },
    );
    RowPartition {
        meta: partition_meta(
            PartitionKey::C4Artifact(artifact.rel_path.clone()),
            c4_artifact_input_fingerprint(artifact),
            PartitionDependencies::default(),
        ),
        rows: graph_rows(graph),
    }
}

fn collect_graph_source_dependencies(graph: &Graph, dependencies: &mut PartitionDependencies) {
    for node in &graph.nodes {
        if node.kind == "architecture-interface"
            && let Some(path) = node
                .path
                .as_deref()
                .and_then(|target| target.split('#').next())
        {
            dependencies.source_content_paths.insert(path.to_string());
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "LikeC4 model projection maintains its recursive graph conversion in one place"
)]
pub(super) fn add_likec4_model_to_graph(
    graph: &mut Graph,
    vault: &Vault,
    model: &serde_json::Value,
) {
    let mut seen_nodes = graph
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    let mut seen_edges = graph
        .edges
        .iter()
        .map(|edge| format!("{}\0{}\0{}", edge.from, edge.to, edge.kind))
        .collect::<BTreeSet<_>>();
    let workspace_id = "architecture:likec4";
    add_node(
        graph,
        &mut seen_nodes,
        Node {
            id: workspace_id.into(),
            hash: String::new(),
            kind: "architecture-workspace".into(),
            label: "LikeC4 architecture".into(),
            path: Some(vault.likec4_workspace.path.clone()),
        },
    );

    for element in model_array(model, "elements") {
        let Some(id) = element.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let element_id = likec4_element_node_id(id);
        add_node(
            graph,
            &mut seen_nodes,
            Node {
                id: element_id.clone(),
                hash: String::new(),
                kind: "architecture-element".into(),
                label: element
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(id)
                    .to_string(),
                path: Some(vault.likec4_workspace.path.clone()),
            },
        );
        add_edge(
            graph,
            &mut seen_edges,
            workspace_id,
            &element_id,
            "contains",
        );
    }

    for relationship in model_array(model, "relationships") {
        let Some(source) = relationship_endpoint(relationship, "source") else {
            continue;
        };
        let Some(target) = relationship_endpoint(relationship, "target") else {
            continue;
        };
        add_edge(
            graph,
            &mut seen_edges,
            &likec4_element_node_id(source),
            &likec4_element_node_id(target),
            "relates",
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
        add_edge(
            graph,
            &mut seen_edges,
            &element_id,
            &code_node_id(&path),
            "references",
        );
        if let Some((target, interface_hash)) = interface_anchor_hash(vault, target, &path) {
            let interface_id = likec4_interface_node_id(element);
            add_node(
                graph,
                &mut seen_nodes,
                Node {
                    id: interface_id.clone(),
                    hash: String::new(),
                    kind: "architecture-interface".into(),
                    label: interface_hash,
                    path: Some(target),
                },
            );
            add_edge(
                graph,
                &mut seen_edges,
                &element_id,
                &interface_id,
                "tracks-interface",
            );
        }
    }

    graph.nodes.sort_by(|left, right| left.id.cmp(&right.id));
    graph.edges.sort_by(|left, right| {
        (&left.from, &left.to, &left.kind).cmp(&(&right.from, &right.to, &right.kind))
    });
    graph.root = graph_root(graph);
}
