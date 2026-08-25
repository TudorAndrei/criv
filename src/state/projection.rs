use super::*;

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
