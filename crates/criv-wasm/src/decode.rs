//! State envelope decoding and schema validation.

use std::fmt::Display;

#[cfg(not(target_arch = "wasm32"))]
use criv_state_wire::STATE_SCHEMA;
use wasm_bindgen::prelude::*;

/// Stable reason a State document cannot be loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateErrorCode {
    JsonInvalid,
    SchemaUnsupported,
}

impl StateErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::JsonInvalid => "criv-state-json-invalid",
            Self::SchemaUnsupported => "criv-state-schema-unsupported",
        }
    }
}

/// A rejected State document, delivered to JavaScript as an `Error` with a `code`.
#[derive(Debug)]
pub struct StateError {
    pub code: StateErrorCode,
    pub message: String,
}

impl StateError {
    pub fn json_invalid(detail: impl Display) -> Self {
        Self {
            code: StateErrorCode::JsonInvalid,
            message: format!("invalid criv state JSON: {detail}"),
        }
    }

    pub fn schema_unsupported(schema: &str) -> Self {
        Self {
            code: StateErrorCode::SchemaUnsupported,
            message: format!("unsupported criv state schema: {schema}"),
        }
    }

    pub fn into_js(self) -> JsValue {
        let error = js_sys::Error::new(&self.message);
        let code_set = js_sys::Reflect::set(
            &error,
            &JsValue::from_str("code"),
            &JsValue::from_str(self.code.as_str()),
        );
        debug_assert!(matches!(code_set, Ok(true)));
        error.into()
    }
}

#[cfg(target_arch = "wasm32")]
pub fn js_error_message(error: &JsValue) -> String {
    js_sys::Reflect::get(error, &JsValue::from_str("message"))
        .ok()
        .and_then(|value| value.as_string())
        .or_else(|| error.as_string())
        .unwrap_or_else(|| "unknown JavaScript error".into())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn decode_state_value(raw: &str) -> Result<serde_json::Value, StateError> {
    let state = serde_json::from_str::<serde_json::Value>(raw).map_err(StateError::json_invalid)?;
    let schema = state
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>");
    if schema != STATE_SCHEMA {
        return Err(StateError::schema_unsupported(schema));
    }
    Ok(state)
}
