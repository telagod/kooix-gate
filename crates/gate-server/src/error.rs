//! 统一错误 → HTTP 响应映射
//!
//! 所有 handler 返回 `Result<_, AppError>`。AppError 是吸纳层：
//! - AuthError 自动映射状态码 (401/403/429)
//! - CoreError 区分 NotFound/Invalid/Conflict
//! - 其他错误统一 500

use axum::Json;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use gate_providers::error::NormalizedProviderErrorKind;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Auth(#[from] gate_auth::AuthError),

    #[error(transparent)]
    Core(#[from] gate_core::CoreError),

    #[error(transparent)]
    Loader(#[from] crate::loader::LoaderError),

    #[error(transparent)]
    Db(#[from] gate_storage::DbError),

    #[error(transparent)]
    Provider(#[from] gate_providers::ProviderError),

    #[error("no route: capability={capability} model={model}")]
    NoRoute {
        capability: &'static str,
        model: String,
    },

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("not found")]
    NotFound,

    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Serialize)]
struct ApiErrorBody {
    error: ApiErrorDetail,
}

#[derive(Serialize)]
struct ApiErrorDetail {
    code: &'static str,
    r#type: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    param: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    upstream_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    upstream_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimension: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capability: Option<&'static str>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let mut retry_after_ms = None;
        let mut upstream_status = None;
        let mut upstream_code = None;
        let mut dimension = None;
        let mut capability = None;
        let (status, code, error_type, msg) = match &self {
            AppError::Auth(e) => {
                let status = StatusCode::from_u16(e.http_status())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                let code = match e {
                    gate_auth::AuthError::MissingCredentials => "missing_credentials",
                    gate_auth::AuthError::InvalidCredentials => "invalid_credentials",
                    gate_auth::AuthError::TokenExpired => "token_expired",
                    gate_auth::AuthError::TokenInvalid(_) => "token_invalid",
                    gate_auth::AuthError::Forbidden { .. } => "forbidden",
                    gate_auth::AuthError::AccountSuspended => "account_suspended",
                    gate_auth::AuthError::ApiKeyRevoked => "api_key_revoked",
                    gate_auth::AuthError::ApiKeyModelNotAllowed(_) => "api_key_model_not_allowed",
                    gate_auth::AuthError::ApiKeyIpNotAllowed => "api_key_ip_not_allowed",
                    gate_auth::AuthError::TooManyFailures => "too_many_failures",
                    gate_auth::AuthError::PasswordTooWeak => "password_too_weak",
                    gate_auth::AuthError::Invalid(_) => "invalid",
                    _ => "internal",
                };
                (
                    status,
                    code,
                    error_type_for_status(status),
                    self.to_string(),
                )
            }
            AppError::Core(gate_core::CoreError::PermissionDenied { .. }) => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "permission_error",
                self.to_string(),
            ),
            AppError::Core(gate_core::CoreError::QuotaExceeded { dimension: dim, .. }) => {
                dimension = Some(dim.clone());
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    "quota_exceeded",
                    "quota_error",
                    self.to_string(),
                )
            }
            AppError::Core(gate_core::CoreError::RateLimited { dimension: dim }) => {
                dimension = Some(dim.clone());
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limited",
                    "rate_limit_error",
                    self.to_string(),
                )
            }
            AppError::Core(gate_core::CoreError::NotFound(_)) => (
                StatusCode::NOT_FOUND,
                "not_found",
                "invalid_request_error",
                self.to_string(),
            ),
            AppError::Core(gate_core::CoreError::Invalid(_)) => (
                StatusCode::BAD_REQUEST,
                "bad_request",
                "invalid_request_error",
                self.to_string(),
            ),
            AppError::Core(gate_core::CoreError::Conflict(_)) => (
                StatusCode::CONFLICT,
                "conflict",
                "invalid_request_error",
                self.to_string(),
            ),
            AppError::Loader(crate::loader::LoaderError::UserUnavailable) => (
                StatusCode::UNAUTHORIZED,
                "user_unavailable",
                "authentication_error",
                self.to_string(),
            ),
            AppError::Loader(crate::loader::LoaderError::ApiKeyInvalid) => (
                StatusCode::UNAUTHORIZED,
                "api_key_invalid",
                "authentication_error",
                self.to_string(),
            ),
            AppError::Loader(crate::loader::LoaderError::ApiKeyRevoked) => (
                StatusCode::FORBIDDEN,
                "api_key_revoked",
                "authentication_error",
                self.to_string(),
            ),
            AppError::Loader(crate::loader::LoaderError::ApiKeyIpDenied) => (
                StatusCode::FORBIDDEN,
                "api_key_ip_denied",
                "permission_error",
                self.to_string(),
            ),
            AppError::Db(gate_storage::DbError::NotFound) => (
                StatusCode::NOT_FOUND,
                "not_found",
                "invalid_request_error",
                "resource not found".into(),
            ),
            AppError::Db(gate_storage::DbError::Conflict(_)) => (
                StatusCode::CONFLICT,
                "conflict",
                "invalid_request_error",
                self.to_string(),
            ),
            AppError::Db(gate_storage::DbError::Constraint(_)) => (
                StatusCode::BAD_REQUEST,
                "constraint_violation",
                "invalid_request_error",
                self.to_string(),
            ),
            AppError::Provider(e) => {
                let normalized = ProviderApiError::from_error(e);
                retry_after_ms = normalized.retry_after_ms;
                upstream_status = normalized.upstream_status;
                upstream_code = normalized.upstream_code.clone();
                (
                    normalized.status,
                    normalized.code,
                    normalized.error_type,
                    normalized.message,
                )
            }
            AppError::NoRoute {
                capability: cap,
                model,
            } => {
                capability = Some(*cap);
                (
                    StatusCode::BAD_REQUEST,
                    "no_healthy_channel",
                    "invalid_request_error",
                    format!("no healthy {cap} channel found for model '{model}'"),
                )
            }
            AppError::BadRequest(_) => (
                StatusCode::BAD_REQUEST,
                "bad_request",
                "invalid_request_error",
                self.to_string(),
            ),
            AppError::Forbidden(_) => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "permission_error",
                self.to_string(),
            ),
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "invalid_request_error",
                "resource not found".into(),
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "server_error",
                "internal error".into(),
            ),
        };

        // 5xx 写日志，4xx 静默
        if status.is_server_error() {
            tracing::error!(error = %self, "request failed");
        } else {
            tracing::debug!(error = %self, "request rejected");
        }

        let mut resp = (
            status,
            Json(ApiErrorBody {
                error: ApiErrorDetail {
                    code,
                    r#type: error_type,
                    message: msg,
                    param: None,
                    retry_after_ms,
                    upstream_status,
                    upstream_code,
                    dimension,
                    capability,
                },
            }),
        )
            .into_response();
        if let Some(ms) = retry_after_ms {
            let secs = ms.div_ceil(1000).max(1);
            resp.headers_mut().insert(
                "retry-after",
                HeaderValue::from_str(&secs.to_string()).unwrap_or(HeaderValue::from_static("1")),
            );
        }
        resp
    }
}

pub type AppResult<T> = std::result::Result<T, AppError>;

struct ProviderApiError {
    status: StatusCode,
    code: &'static str,
    error_type: &'static str,
    message: String,
    retry_after_ms: Option<u64>,
    upstream_status: Option<u16>,
    upstream_code: Option<String>,
}

impl ProviderApiError {
    fn from_error(error: &gate_providers::ProviderError) -> Self {
        use gate_providers::ProviderError;
        match error {
            ProviderError::Auth(_) => Self {
                status: StatusCode::BAD_GATEWAY,
                code: "authentication_error",
                error_type: "authentication_error",
                message: "upstream auth failed".to_string(),
                retry_after_ms: None,
                upstream_status: Some(401),
                upstream_code: None,
            },
            ProviderError::RateLimited { retry_after_ms } => Self {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "rate_limit_error",
                error_type: "rate_limit_error",
                message: "upstream rate limited".to_string(),
                retry_after_ms: *retry_after_ms,
                upstream_status: Some(429),
                upstream_code: None,
            },
            ProviderError::ModelNotFound(message) => Self {
                status: StatusCode::NOT_FOUND,
                code: "model_not_found",
                error_type: "invalid_request_error",
                message: sanitize_upstream_message(message, "upstream model not found"),
                retry_after_ms: None,
                upstream_status: Some(404),
                upstream_code: None,
            },
            ProviderError::InvalidRequest(message) => Self {
                status: StatusCode::BAD_REQUEST,
                code: "invalid_request_error",
                error_type: "invalid_request_error",
                message: sanitize_upstream_message(message, "upstream invalid request"),
                retry_after_ms: None,
                upstream_status: Some(400),
                upstream_code: None,
            },
            ProviderError::Policy(message) => Self {
                status: StatusCode::FORBIDDEN,
                code: "policy_error",
                error_type: "policy_error",
                message: sanitize_upstream_message(message, "upstream policy blocked"),
                retry_after_ms: None,
                upstream_status: Some(403),
                upstream_code: None,
            },
            ProviderError::Mapped {
                status,
                code,
                message,
                metadata,
            } => from_mapped_provider_error(*status, code.clone(), message, metadata),
            ProviderError::Network(_) => Self {
                status: StatusCode::BAD_GATEWAY,
                code: "upstream_unreachable",
                error_type: "upstream_error",
                message: "upstream unreachable".to_string(),
                retry_after_ms: None,
                upstream_status: None,
                upstream_code: None,
            },
            ProviderError::Decode(_) => Self {
                status: StatusCode::BAD_GATEWAY,
                code: "upstream_decode_error",
                error_type: "upstream_error",
                message: "upstream response decode failed".to_string(),
                retry_after_ms: None,
                upstream_status: None,
                upstream_code: None,
            },
            ProviderError::Config(_) => Self {
                status: StatusCode::BAD_GATEWAY,
                code: "upstream_config_error",
                error_type: "upstream_error",
                message: "upstream provider config error".to_string(),
                retry_after_ms: None,
                upstream_status: None,
                upstream_code: None,
            },
            ProviderError::Upstream { status, body } => {
                let http_status = StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY);
                Self {
                    status: if http_status.is_client_error() {
                        http_status
                    } else {
                        StatusCode::BAD_GATEWAY
                    },
                    code: "upstream_error",
                    error_type: "upstream_error",
                    message: sanitize_upstream_message(body, "upstream request failed"),
                    retry_after_ms: None,
                    upstream_status: Some(*status),
                    upstream_code: None,
                }
            }
        }
    }
}

pub(crate) struct ProviderFailurePolicy {
    pub kind_label: &'static str,
    pub reason: String,
    pub error_code: Option<i32>,
    pub cooldown_secs: i64,
    pub circuit_breaker_failures: u32,
}

pub(crate) fn provider_failure_policy(
    error: &gate_providers::ProviderError,
) -> ProviderFailurePolicy {
    use gate_providers::ProviderError;
    let normalized = ProviderApiError::from_error(error);
    let (reason, cooldown_ms, circuit_breaker_failures) = match error {
        ProviderError::Auth(message)
        | ProviderError::ModelNotFound(message)
        | ProviderError::InvalidRequest(message)
        | ProviderError::Policy(message)
        | ProviderError::Network(message)
        | ProviderError::Decode(message)
        | ProviderError::Config(message) => (message.clone(), None, None),
        ProviderError::RateLimited { retry_after_ms } => (error.to_string(), *retry_after_ms, None),
        ProviderError::Upstream { status, body } => {
            (body.clone(), status.ge(&500).then_some(60_000), None)
        }
        ProviderError::Mapped {
            message, metadata, ..
        } => (
            message.clone(),
            metadata.cooldown_ms.or(metadata.retry_after_ms),
            metadata.circuit_breaker_failures,
        ),
    };

    ProviderFailurePolicy {
        kind_label: normalized.code,
        reason: format!(
            "{}: {}",
            normalized.code,
            sanitize_upstream_message(&reason, &normalized.message)
        ),
        error_code: normalized
            .upstream_status
            .map(i32::from)
            .or_else(|| Some(normalized.status.as_u16().into())),
        cooldown_secs: cooldown_ms
            .map(|ms| ms.div_ceil(1000).max(1) as i64)
            .unwrap_or(300),
        circuit_breaker_failures: circuit_breaker_failures.unwrap_or(3).max(1),
    }
}

fn from_mapped_provider_error(
    upstream_status: Option<u16>,
    upstream_code: Option<String>,
    message: &str,
    metadata: &gate_providers::error::ProviderErrorMetadata,
) -> ProviderApiError {
    let retry_after_ms = metadata.retry_after_ms.or(metadata.cooldown_ms);
    let status_from_upstream = upstream_status.and_then(|status| StatusCode::from_u16(status).ok());
    let status = match metadata.kind {
        NormalizedProviderErrorKind::Authentication => StatusCode::BAD_GATEWAY,
        NormalizedProviderErrorKind::RateLimit => StatusCode::TOO_MANY_REQUESTS,
        NormalizedProviderErrorKind::ModelNotFound => StatusCode::NOT_FOUND,
        NormalizedProviderErrorKind::InvalidRequest => StatusCode::BAD_REQUEST,
        NormalizedProviderErrorKind::Policy => StatusCode::FORBIDDEN,
        NormalizedProviderErrorKind::Upstream => status_from_upstream
            .filter(|status| status.is_client_error())
            .unwrap_or(StatusCode::BAD_GATEWAY),
    };
    let (code, error_type, fallback_message) = match metadata.kind {
        NormalizedProviderErrorKind::Authentication => (
            "authentication_error",
            "authentication_error",
            "upstream auth failed",
        ),
        NormalizedProviderErrorKind::RateLimit => (
            "rate_limit_error",
            "rate_limit_error",
            "upstream rate limited",
        ),
        NormalizedProviderErrorKind::ModelNotFound => (
            "model_not_found",
            "invalid_request_error",
            "upstream model not found",
        ),
        NormalizedProviderErrorKind::InvalidRequest => (
            "invalid_request_error",
            "invalid_request_error",
            "upstream invalid request",
        ),
        NormalizedProviderErrorKind::Policy => {
            ("policy_error", "policy_error", "upstream policy blocked")
        }
        NormalizedProviderErrorKind::Upstream => (
            "upstream_error",
            "upstream_error",
            "upstream request failed",
        ),
    };
    let code = match (
        metadata.kind,
        upstream_code.as_deref() == Some("no_healthy_channel"),
    ) {
        (NormalizedProviderErrorKind::ModelNotFound, true) => "no_healthy_channel",
        _ => code,
    };
    ProviderApiError {
        status,
        code,
        error_type,
        message: sanitize_upstream_message(message, fallback_message),
        retry_after_ms,
        upstream_status,
        upstream_code,
    }
}

fn sanitize_upstream_message(message: &str, fallback: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.chars().take(512).collect()
    }
}

fn error_type_for_status(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => "authentication_error",
        StatusCode::FORBIDDEN => "permission_error",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
        status if status.is_client_error() => "invalid_request_error",
        status if status.is_server_error() => "server_error",
        _ => "server_error",
    }
}
