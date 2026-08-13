//! LikeC4 contract validation and editor projection.

use std::collections::{BTreeMap, BTreeSet};

use criv_state_wire::LikeC4ArchitectureState;
use serde::Deserialize;

use super::{
    EditorC4Artifact, EditorGraphNode, EditorLikeC4Model, EditorSourceEntry, PublishedLikeC4Model,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LikeC4Contract {
    protocol_version: u32,
    likec4_version: String,
}

pub(super) fn prepare_architecture(
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
    let workspace = super::source::safe_source_path(&architecture.workspace).ok_or_else(|| {
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
                .is_some_and(|path| super::source::safe_source_path(path).is_none())
        {
            return Err("criv-likec4-model-invalid: invalid named view".into());
        }
    }
    for link in &published.source_links {
        let path = link
            .target
            .split_once('#')
            .map_or(link.target.as_str(), |value| value.0);
        if link.element.trim().is_empty() || super::source::safe_source_path(path).is_none() {
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

pub(super) fn prepare_c4_artifacts(
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
