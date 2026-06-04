//! Provider 错误。
//!
//! - `Upstream`：上游返回非 2xx，带 status code（透传给客户端时挑合适映射）
//! - `Network`：连不上、超时、TLS
//! - `Decode`：拿到 200 但 JSON 解不出
//! - `Auth`：上游 401/403（key 失效）
//!
//! ## Body 脱敏（0.4.69，product-review A5）
//!
//! `Upstream { body }` 与 `Mapped { message }` 都可能包含上游响应原文。
//! 上游 4xx/5xx 偶尔会回显请求体片段（OpenAI tool_use error 已知会回显
//! 参数）或敏感 header echo —— 直接进 audit / log sink 会泄漏 PII / key。
//!
//! 解决方案：用 [`redact_upstream_body`] 截 512 字节 + 末尾哈希，配合 server 层
//! `audit_redaction` 进一步过滤；构造点统一走 [`ProviderError::upstream`] 工厂。

use thiserror::Error;

/// 0.4.69: Provider error body 进入 log/audit/客户端响应前的最大保留长度。
/// 超过部分截断，并附 SHA-256 哈希前 16 字符方便事后定位完整内容。
const ERROR_BODY_KEEP_BYTES: usize = 512;

/// 0.4.69：脱敏上游错误 body。
/// - 长度 ≤ 512 字节：原样保留
/// - 超过：保留前 512 字节 + 标注被截断的字节数 + body 整体 SHA-256 前 16 字符
///
/// 不做内容过滤（如 key 检测）—— 那是 server 层 `audit_redaction` 的职责。
/// 这里只防"长 body 撑爆日志 / 拷贝放大内存压力"。
pub fn redact_upstream_body(body: &str) -> String {
    if body.len() <= ERROR_BODY_KEEP_BYTES {
        return body.to_string();
    }
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(body.as_bytes());
    let digest = h.finalize();
    let digest_hex: String = digest
        .iter()
        .take(8)
        .map(|b| format!("{:02x}", b))
        .collect();
    let truncated_at = ERROR_BODY_KEEP_BYTES.min(body.len());
    // 在合法 UTF-8 边界截断
    let mut keep_end = truncated_at;
    while keep_end > 0 && !body.is_char_boundary(keep_end) {
        keep_end -= 1;
    }
    let kept = &body[..keep_end];
    let dropped = body.len().saturating_sub(keep_end);
    format!("{kept}…[truncated {dropped} bytes; sha256={digest_hex}]")
}

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

impl ProviderError {
    /// 0.4.69：构造 `Upstream` error，body 自动经 [`redact_upstream_body`] 脱敏。
    /// 所有 provider 在拿到上游 4xx/5xx body 后都应走此构造函数，保证日志 /
    /// audit / 错误响应里不会出现完整原始 body。
    pub fn upstream(status: u16, body: impl Into<String>) -> Self {
        Self::Upstream {
            status,
            body: redact_upstream_body(&body.into()),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_short_body_is_passthrough() {
        let body = "{\"error\":\"boom\"}";
        assert_eq!(redact_upstream_body(body), body);
    }

    #[test]
    fn redact_long_body_truncates_with_hash() {
        let body = "x".repeat(2048);
        let r = redact_upstream_body(&body);
        assert!(r.starts_with(&"x".repeat(ERROR_BODY_KEEP_BYTES)));
        assert!(r.contains("[truncated"));
        assert!(r.contains("sha256="));
        assert!(r.len() < body.len());
    }

    #[test]
    fn upstream_factory_redacts_long_body() {
        let body = "y".repeat(8192);
        let err = ProviderError::upstream(502, body);
        match err {
            ProviderError::Upstream { status, body } => {
                assert_eq!(status, 502);
                assert!(body.len() < 8192, "body should be truncated");
                assert!(body.contains("sha256="));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn redact_handles_utf8_boundary() {
        // 把多字节 UTF-8 边界跨过 512 字节
        let mut s = "a".repeat(510);
        s.push_str("中文测试");
        let r = redact_upstream_body(&s);
        // 不能 panic，且产物是合法 UTF-8
        assert!(r.is_char_boundary(0));
        assert!(r.contains("[truncated"));
    }
}
