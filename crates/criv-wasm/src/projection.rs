//! Initial editor projection encoding.

#[cfg(not(target_arch = "wasm32"))]
use std::collections::BTreeMap;

#[cfg(not(target_arch = "wasm32"))]
use criv_state_wire::PatternMatch;
use serde::Serialize;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use super::PreparedState;
#[cfg(not(target_arch = "wasm32"))]
use super::{
    EditorAssetEntry, EditorC4Artifact, EditorGraphNode, EditorLikeC4Model, EditorSourceEntry,
    StateSummary,
};

#[cfg(target_arch = "wasm32")]
pub(super) fn initial_projections_from_js(prepared: &PreparedState) -> Result<JsValue, JsValue> {
    let projections = js_sys::Object::new();
    set_js_field(
        &projections,
        "summary",
        &js_projection_value(&prepared.summary)?,
    )?;
    set_js_field(
        &projections,
        "sources",
        &js_projection_value(&prepared.sources)?,
    )?;
    set_js_field(
        &projections,
        "assets",
        &js_projection_value(&prepared.assets)?,
    )?;
    set_js_field(
        &projections,
        "nodes",
        &js_projection_value(&prepared.nodes)?,
    )?;
    set_js_field(
        &projections,
        "registeredPatterns",
        &js_projection_value(&prepared.registered_patterns)?,
    )?;
    set_js_field(
        &projections,
        "patternMatches",
        &js_projection_value(&prepared.pattern_matches)?,
    )?;
    set_js_field(
        &projections,
        "architecture",
        &js_projection_value(&prepared.architecture)?,
    )?;
    set_js_field(
        &projections,
        "c4Artifacts",
        &js_projection_value(&prepared.c4_artifacts)?,
    )?;
    Ok(projections.into())
}

#[cfg(target_arch = "wasm32")]
fn set_js_field(object: &js_sys::Object, name: &str, value: &JsValue) -> Result<(), JsValue> {
    js_sys::Reflect::set(object, &JsValue::from_str(name), value)
        .map(|_| ())
        .map_err(|error| {
            JsValue::from_str(&format!(
                "failed to encode criv initial projections: {}",
                super::decode::js_error_message(&error)
            ))
        })
}

#[cfg(target_arch = "wasm32")]
fn js_encode_error(error: serde_wasm_bindgen::Error) -> JsValue {
    JsValue::from_str(&format!(
        "failed to encode criv initial projections: {error}"
    ))
}

#[cfg(target_arch = "wasm32")]
fn js_projection_value(value: &impl Serialize) -> Result<JsValue, JsValue> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
        .map_err(js_encode_error)
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Serialize)]
pub(super) struct InitialProjections<'a> {
    summary: &'a StateSummary,
    sources: &'a [EditorSourceEntry],
    assets: &'a [EditorAssetEntry],
    nodes: &'a [EditorGraphNode],
    #[serde(rename = "registeredPatterns")]
    registered_patterns: &'a [String],
    #[serde(rename = "patternMatches")]
    pattern_matches: &'a BTreeMap<String, Vec<PatternMatch>>,
    architecture: &'a Option<EditorLikeC4Model>,
    #[serde(rename = "c4Artifacts")]
    c4_artifacts: &'a [EditorC4Artifact],
}

#[cfg(not(target_arch = "wasm32"))]
impl<'a> From<&'a PreparedState> for InitialProjections<'a> {
    fn from(prepared: &'a PreparedState) -> Self {
        Self {
            summary: &prepared.summary,
            sources: &prepared.sources,
            assets: &prepared.assets,
            nodes: &prepared.nodes,
            registered_patterns: &prepared.registered_patterns,
            pattern_matches: &prepared.pattern_matches,
            architecture: &prepared.architecture,
            c4_artifacts: &prepared.c4_artifacts,
        }
    }
}
