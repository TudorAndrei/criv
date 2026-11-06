//! State envelope decoding and schema validation.

use std::fmt::Display;

use criv_state_wire::{StateDocument, StateReadError};
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

/// Decode one State document after checking its schema.
pub fn decode_state(raw: &str) -> Result<StateDocument, StateError> {
    criv_state_wire::decode(raw).map_err(|error| match error {
        StateReadError::Json(error) => StateError::json_invalid(error),
        StateReadError::UnsupportedSchema(schema) => StateError::schema_unsupported(&schema),
    })
}
