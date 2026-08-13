//! State envelope decoding and schema validation.

use criv_state_wire::LikeC4ArchitectureState;
#[cfg(not(target_arch = "wasm32"))]
use criv_state_wire::is_supported_schema;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
pub(super) fn validate_architecture_wrapper_wasm(envelope: &JsValue) -> Result<(), JsValue> {
    let architecture = js_sys::Reflect::get(envelope, &JsValue::from_str("architecture"))
        .unwrap_or(JsValue::UNDEFINED);
    if architecture.is_undefined() || architecture.is_null() {
        return Ok(());
    }
    serde_wasm_bindgen::from_value::<LikeC4ArchitectureState>(architecture)
        .map(|_| ())
        .map_err(|error| JsValue::from_str(&format!("criv-likec4-architecture-invalid: {error}")))
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn validate_architecture_wrapper(envelope: &serde_json::Value) -> Result<(), String> {
    let Some(architecture) = envelope
        .get("architecture")
        .filter(|value| !value.is_null())
    else {
        return Ok(());
    };
    serde_json::from_value::<LikeC4ArchitectureState>(architecture.clone())
        .map(|_| ())
        .map_err(|error| format!("criv-likec4-architecture-invalid: {error}"))
}

#[cfg(target_arch = "wasm32")]
pub(super) fn js_error_message(error: &JsValue) -> String {
    js_sys::Reflect::get(error, &JsValue::from_str("message"))
        .ok()
        .and_then(|value| value.as_string())
        .or_else(|| error.as_string())
        .unwrap_or_else(|| "unknown JavaScript error".into())
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn decode_state_value(raw: &str) -> Result<serde_json::Value, String> {
    let state = serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|err| format!("criv-state-json-invalid: {err}"))?;
    let schema = state
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>");
    if !is_supported_schema(schema) {
        return Err(format!(
            "criv-state-schema-unsupported: unsupported criv state schema: {schema}"
        ));
    }
    Ok(state)
}
