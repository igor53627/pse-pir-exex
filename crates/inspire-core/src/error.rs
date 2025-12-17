//! Error types for inspire-core

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    #[error("Contract not found: {0}")]
    ContractNotFound(String),

    #[error("Index out of bounds: {index} >= {max}")]
    IndexOutOfBounds { index: u64, max: u64 },

    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("Lane mismatch: expected {expected}, got {actual}")]
    LaneMismatch { expected: String, actual: String },
}
