//! Error types for the embedding service.

use thiserror::Error;

/// Service-level errors.
#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("transport error: {0}")]
    Transport(String),

    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("connection closed by peer")]
    ConnectionClosed,

    #[error("request timeout after {0:?}")]
    Timeout(std::time::Duration),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("backend error: {0}")]
    Backend(String),

    #[error("server is shutting down")]
    ShuttingDown,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

impl ServiceError {
    /// Map to a JSON-RPC error code.
    pub fn to_rpc_code(&self) -> i32 {
        use crate::protocol::error_code;
        match self {
            ServiceError::Protocol(_) => error_code::INVALID_REQUEST,
            ServiceError::Backend(_) => error_code::BACKEND_ERROR,
            ServiceError::ShuttingDown => error_code::SERVER_SHUTTING_DOWN,
            ServiceError::Timeout(_) => error_code::INTERNAL_ERROR,
            ServiceError::ConnectionClosed => error_code::INTERNAL_ERROR,
            ServiceError::ConnectionFailed(_) => error_code::INTERNAL_ERROR,
            ServiceError::Transport(_) => error_code::INTERNAL_ERROR,
            ServiceError::Io(_) => error_code::INTERNAL_ERROR,
            ServiceError::Json(_) => error_code::PARSE_ERROR,
        }
    }
}

impl From<ServiceError> for std::io::Error {
    fn from(e: ServiceError) -> Self {
        match e {
            ServiceError::Io(io_err) => io_err,
            ServiceError::Timeout(d) => {
                std::io::Error::new(std::io::ErrorKind::TimedOut, format!("timeout after {d:?}"))
            }
            ServiceError::ConnectionClosed => {
                std::io::Error::new(std::io::ErrorKind::ConnectionReset, "connection closed")
            }
            other => std::io::Error::new(std::io::ErrorKind::Other, other.to_string()),
        }
    }
}
