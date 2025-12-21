//! Error types for WASM PIR client

use wasm_bindgen::prelude::*;

#[derive(Debug)]
pub enum PirError {
    Network(String),
    Serialization(String),
    Pir(String),
    NotInitialized,
    IndexOutOfBounds(u64),
    VersionMismatch { client: u16, server: u16 },
}

impl std::fmt::Display for PirError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PirError::Network(msg) => write!(f, "Network error: {}", msg),
            PirError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            PirError::Pir(msg) => write!(f, "PIR error: {}", msg),
            PirError::NotInitialized => write!(f, "Client not initialized"),
            PirError::IndexOutOfBounds(idx) => write!(f, "Index {} out of bounds", idx),
            PirError::VersionMismatch { client, server } => {
                write!(
                    f,
                    "PIR params version mismatch: client v{}, server v{}. Update client or regenerate server CRS.",
                    client, server
                )
            }
        }
    }
}

impl std::error::Error for PirError {}

impl From<PirError> for JsValue {
    fn from(err: PirError) -> Self {
        JsValue::from_str(&err.to_string())
    }
}

impl From<gloo_net::Error> for PirError {
    fn from(err: gloo_net::Error) -> Self {
        PirError::Network(err.to_string())
    }
}

impl From<serde_json::Error> for PirError {
    fn from(err: serde_json::Error) -> Self {
        PirError::Serialization(err.to_string())
    }
}
