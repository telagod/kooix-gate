//! Provider 错误。
//!
//! - `Upstream`：上游返回非 2xx，带 status code（透传给客户端时挑合适映射）
//! - `Network`：连不上、超时、TLS
//! - `Decode`：拿到 200 但 JSON 解不出
//! - `Auth`：上游 401/403（key 失效）

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizedProviderErrorKind {
    Authentication,
    RateLimit,
    ModelNotFound,
    InvalidRequest,
    Policy,
    Upstream,
}

#[derive(Debug, Clone)]
pub struct ProviderErrorMetadata {
    pub kind: NormalizedProviderErrorKind,
    pub retryable: bool,
    pub cooldown_ms: Option<u64>,
    pub circuit_breaker_failures: Option<u32>,
    pub retry_after_ms: Option<u64>,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("upstream {status}: {body}")]
    Upstream { status: u16, body: String },

    #[error("upstream rate limited: retry_after_ms={retry_after_ms:?}")]
    RateLimited { retry_after_ms: Option<u64> },

    #[error("upstream auth failed: {0}")]
    Auth(String),

    #[error("upstream invalid request: {0}")]
    InvalidRequest(String),

    #[error("upstream model not found: {0}")]
    ModelNotFound(String),

    #[error("upstream policy blocked: {0}")]
    Policy(String),

    #[error("upstream mapped error: status={status:?} code={code:?} message={message}")]
    Mapped {
        status: Option<u16>,
        code: Option<String>,
        message: String,
        metadata: ProviderErrorMetadata,
    },

    #[error("network: {0}")]
    Network(String),

    #[error("decode: {0}")]
    Decode(String),

    #[error("config: {0}")]
    Config(String),
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        if let Some(status) = e.status() {
            let code = status.as_u16();
            return match code {
                401 | 403 => Self::Auth(format!("upstream returned {code}")),
                404 => Self::ModelNotFound(format!("upstream returned {code}")),
                429 => Self::RateLimited {
                    retry_after_ms: None,
                },
                400..=499 => Self::InvalidRequest(format!("upstream returned {code}")),
                _ => Self::Upstream {
                    status: code,
                    body: format!("upstream returned {code}"),
                },
            };
        }
        if e.is_timeout() || e.is_connect() {
            Self::Network(e.to_string())
        } else if e.is_decode() {
            Self::Decode(e.to_string())
        } else {
            Self::Network(e.to_string())
        }
    }
}

impl From<serde_json::Error> for ProviderError {
    fn from(e: serde_json::Error) -> Self {
        Self::Decode(e.to_string())
    }
}

pub type ProviderResult<T> = Result<T, ProviderError>;
