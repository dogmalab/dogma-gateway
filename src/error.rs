//! Typed errors for dogma-gateway.
//!
//! Every network or JSON parsing error is mapped into [`GatewayError`] and
//! converted to an HTTP response via [`IntoResponse`].
//!
//! # Safety
//! 0 `unsafe` in this module.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Gateway-level error that implements [`IntoResponse`] for Axum handlers.
///
/// All variants map to appropriate HTTP status codes. The error type is
/// designed to be constructed exclusively via `?` (using [`From`] impls or
/// direct constructors) — never via `unwrap()` or `expect()`.
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum GatewayError {
    /// The request payload failed schema validation.
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// The requested resource does not exist.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Internal server failure (I/O, downstream service, etc.).
    #[error("Internal error: {0}")]
    Internal(String),

    /// JSON serialization / deserialization failure.
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// I/O error (IPC pipes, filesystem, network bind).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "BAD_REQUEST"),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
            Self::Serde(_) => (StatusCode::BAD_REQUEST, "PARSE_ERROR"),
            Self::Io(_) => (StatusCode::INTERNAL_SERVER_ERROR, "IO_ERROR"),
        };

        let body = Json(json!({
            "error": code,
            "message": self.to_string(),
        }));

        (status, body).into_response()
    }
}
