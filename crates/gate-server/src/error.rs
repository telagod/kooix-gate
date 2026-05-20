//! 统一错误 → HTTP 响应映射
//!
//! 所有 handler 返回 `Result<_, AppError>`。AppError 是吸纳层：
//! - AuthError 自动映射状态码 (401/403/429)
//! - CoreError 区分 NotFound/Invalid/Conflict
//! - 其他错误统一 500

use crate::provider_error::ProviderApiError;
pub(crate) use crate::provider_error::provider_failure_policy;
use axum::Json;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
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
