use thiserror::Error;

pub type Result<T> = std::result::Result<T, AuthError>;

#[derive(Debug, Error)]
pub enum AuthError {
    // 401
    #[error("missing credentials")]
    MissingCredentials,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("token expired")]
    TokenExpired,
    #[error("token invalid: {0}")]
    TokenInvalid(String),

    // 403
    #[error("permission denied: {action} on {resource}")]
    Forbidden { action: String, resource: String },
    #[error("account suspended")]
    AccountSuspended,
    #[error("api key revoked")]
    ApiKeyRevoked,
    #[error("api key model not allowed: {0}")]
    ApiKeyModelNotAllowed(String),
    #[error("api key ip not allowed")]
    ApiKeyIpNotAllowed,

    // 429
    #[error("too many login failures")]
    TooManyFailures,

    // 400
    #[error("password too weak")]
    PasswordTooWeak,
    #[error("invalid input: {0}")]
    Invalid(String),

    // 500
    #[error("password hash error: {0}")]
    Hash(String),
    #[error("crypto: {0}")]
    Crypto(#[from] gate_crypto::CryptoError),
    #[error("jwt: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
    #[error("oidc: {0}")]
    Oidc(String),
    #[error("internal: {0}")]
    Internal(String),
}

impl AuthError {
    /// 建议的 HTTP 状态码
    pub fn http_status(&self) -> u16 {
        use AuthError::*;
        match self {
            MissingCredentials | InvalidCredentials | TokenExpired | TokenInvalid(_) => 401,
            Forbidden { .. } | AccountSuspended | ApiKeyRevoked
            | ApiKeyModelNotAllowed(_) | ApiKeyIpNotAllowed => 403,
            TooManyFailures => 429,
            PasswordTooWeak | Invalid(_) => 400,
            Hash(_) | Crypto(_) | Jwt(_) | Oidc(_) | Internal(_) => 500,
        }
    }
}

impl From<AuthError> for gate_core::CoreError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::Forbidden { action, resource } => {
                gate_core::CoreError::PermissionDenied { action, resource }
            }
            other => gate_core::CoreError::Internal(other.to_string()),
        }
    }
}
