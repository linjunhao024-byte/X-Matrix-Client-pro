use serde::Serialize;
use specta::Type;

/// Every command returns this error type. Structured, never a bare string —
/// the frontend must be able to branch on `kind` without parsing prose.
#[derive(Debug, Clone, Serialize, Type, thiserror::Error)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Engine(String),
    #[error("{0}")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Internal(format!("json: {e}"))
    }
}
