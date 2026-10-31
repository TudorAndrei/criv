//! State envelope decoding and schema validation.

#[cfg(not(target_arch = "wasm32"))]
use criv_state_wire::STATE_SCHEMA;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
pub fn js_error_message(error: &JsValue) -> String {
    js_sys::Reflect::get(error, &JsValue::from_str("message"))
        .ok()
        .and_then(|value| value.as_string())
        .or_else(|| error.as_string())
        .unwrap_or_else(|| "unknown JavaScript error".into())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn decode_state_value(raw: &str) -> Result<serde_json::Value, String> {
    let state = serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|err| format!("criv-state-json-invalid: {err}"))?;
    let schema = state
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>");
    if schema != STATE_SCHEMA {
        return Err(format!(
            "criv-state-schema-unsupported: unsupported criv state schema: {schema}"
        ));
    }
    Ok(state)
}
