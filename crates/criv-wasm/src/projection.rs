//! Initial editor projection encoding.

use std::collections::BTreeMap;

use criv_state_wire::{AssetIndexEntry, PatternMatch, SourceIndexEntry};
use serde::Serialize;
use wasm_bindgen::JsValue;

use super::{EditorGraphNode, PreparedState, StateSummary};

/// The projections JavaScript reads once after a load.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialProjections<'a> {
    summary: &'a StateSummary,
    sources: &'a [SourceIndexEntry],
    assets: &'a [AssetIndexEntry],
    nodes: &'a [EditorGraphNode],
    registered_patterns: &'a [String],
    pattern_matches: &'a BTreeMap<String, Vec<PatternMatch>>,
}

impl<'a> From<&'a PreparedState> for InitialProjections<'a> {
    fn from(prepared: &'a PreparedState) -> Self {
        Self {
            summary: &prepared.summary,
            sources: &prepared.sources,
            assets: &prepared.assets,
            nodes: &prepared.nodes,
            registered_patterns: &prepared.registered_patterns,
            pattern_matches: &prepared.pattern_matches,
        }
    }
}

impl InitialProjections<'_> {
    /// Encode the projections, with maps as plain JavaScript objects.
    pub fn to_js(&self) -> Result<JsValue, JsValue> {
        self.serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
            .map_err(|error| {
                JsValue::from_str(&format!(
                    "failed to encode criv initial projections: {error}"
                ))
            })
    }
}
