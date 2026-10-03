use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TentacleError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Docker API error: {0}")]
    Docker(#[from] bollard::errors::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Path traversal security violation: {0}")]
    PathTraversal(String),

    #[error("Server not found: {0}")]
    ServerNotFound(String),

    #[error("Container not found: {0}")]
    ContainerNotFound(String),

    #[error("Invalid server state transition: {0}")]
    InvalidState(String),

    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Storage quota exceeded: {0}")]
    QuotaExceeded(String),

    #[error("Archive error: {0}")]
    Archive(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Internal daemon error: {0}")]
    Internal(String),

    #[error("Update error: {0}")]
    Update(String),
}

impl IntoResponse for TentacleError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            TentacleError::ServerNotFound(_) | TentacleError::ContainerNotFound(_) => {
                (StatusCode::NOT_FOUND, self.to_string())
            }
            TentacleError::PathTraversal(_) | TentacleError::Auth(_) => {
                (StatusCode::FORBIDDEN, self.to_string())
            }
            TentacleError::QuotaExceeded(_) => (StatusCode::PAYLOAD_TOO_LARGE, self.to_string()),
            TentacleError::InvalidState(_) | TentacleError::Config(_) | TentacleError::Update(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = axum::Json(json!({
            "success": false,
            "error": error_message
        }));

        (status, body).into_response()
    }
}
