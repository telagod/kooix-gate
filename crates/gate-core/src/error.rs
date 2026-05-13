use thiserror::Error;

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("permission denied: {action} on {resource}")]
    PermissionDenied { action: String, resource: String },

    #[error("quota exceeded: {dimension} ({current}/{limit})")]
    QuotaExceeded {
        dimension: String,
        current: String,
        limit: String,
    },

    #[error("rate limited: {dimension}")]
    RateLimited { dimension: String },

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("internal: {0}")]
    Internal(String),
}
