//! Server error types

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServerError {
    #[error("Lane not loaded: {0}")]
    LaneNotLoaded(String),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("PIR error: {0}")]
    PirError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ServerError::LaneNotLoaded(_) => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            ServerError::InvalidQuery(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ServerError::PirError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            ServerError::Io(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            ServerError::Json(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ServerError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        (status, message).into_response()
    }
}

pub type Result<T> = std::result::Result<T, ServerError>;
