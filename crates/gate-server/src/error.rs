//! 统一错误 → HTTP 响应映射
//!
//! 所有 handler 返回 `Result<_, AppError>`。AppError 是吸纳层：
//! - AuthError 自动映射状态码 (401/403/429)
//! - CoreError 区分 NotFound/Invalid/Conflict
//! - 其他错误统一 500

use axum::Json;
use axum::http::StatusCode;
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

    #[error("bad request: {0}")]
    BadRequest(String),

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
    message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, msg) = match &self {
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
                (status, code, self.to_string())
            }
            AppError::Core(gate_core::CoreError::PermissionDenied { .. }) => {
                (StatusCode::FORBIDDEN, "forbidden", self.to_string())
            }
            AppError::Core(gate_core::CoreError::QuotaExceeded { .. }) => (
                StatusCode::PAYMENT_REQUIRED,
                "quota_exceeded",
                self.to_string(),
            ),
            AppError::Core(gate_core::CoreError::RateLimited { .. }) => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                self.to_string(),
            ),
            AppError::Core(gate_core::CoreError::NotFound(_)) => {
                (StatusCode::NOT_FOUND, "not_found", self.to_string())
            }
            AppError::Core(gate_core::CoreError::Invalid(_)) => {
                (StatusCode::BAD_REQUEST, "bad_request", self.to_string())
            }
            AppError::Core(gate_core::CoreError::Conflict(_)) => {
                (StatusCode::CONFLICT, "conflict", self.to_string())
            }
            AppError::Loader(crate::loader::LoaderError::UserUnavailable) => (
                StatusCode::UNAUTHORIZED,
                "user_unavailable",
                self.to_string(),
            ),
            AppError::Loader(crate::loader::LoaderError::ApiKeyInvalid) => (
                StatusCode::UNAUTHORIZED,
                "api_key_invalid",
                self.to_string(),
            ),
            AppError::Loader(crate::loader::LoaderError::ApiKeyRevoked) => {
                (StatusCode::FORBIDDEN, "api_key_revoked", self.to_string())
            }
            AppError::Loader(crate::loader::LoaderError::ApiKeyIpDenied) => {
                (StatusCode::FORBIDDEN, "api_key_ip_denied", self.to_string())
            }
            AppError::Db(gate_storage::DbError::NotFound) => (
                StatusCode::NOT_FOUND,
                "not_found",
                "resource not found".into(),
            ),
            AppError::Db(gate_storage::DbError::Conflict(_)) => {
                (StatusCode::CONFLICT, "conflict", self.to_string())
            }
            AppError::Db(gate_storage::DbError::Constraint(_)) => (
                StatusCode::BAD_REQUEST,
                "constraint_violation",
                self.to_string(),
            ),
            AppError::Provider(gate_providers::ProviderError::Auth(_)) => (
                StatusCode::BAD_GATEWAY,
                "upstream_auth_failed",
                "upstream auth failed".into(),
            ),
            AppError::Provider(gate_providers::ProviderError::RateLimited { .. }) => (
                StatusCode::TOO_MANY_REQUESTS,
                "upstream_rate_limited",
                self.to_string(),
            ),
            AppError::Provider(gate_providers::ProviderError::Network(_)) => (
                StatusCode::BAD_GATEWAY,
                "upstream_unreachable",
                self.to_string(),
            ),
            AppError::Provider(gate_providers::ProviderError::Decode(_)) => {
                (StatusCode::BAD_GATEWAY, "upstream_decode", self.to_string())
            }
            AppError::Provider(gate_providers::ProviderError::Upstream { status, .. }) => {
                let s = StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY);
                (s, "upstream_error", self.to_string())
            }
            AppError::Provider(_) => (StatusCode::BAD_GATEWAY, "upstream_error", self.to_string()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request", self.to_string()),
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "resource not found".into(),
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal error".into(),
            ),
        };

        // 5xx 写日志，4xx 静默
        if status.is_server_error() {
            tracing::error!(error = %self, "request failed");
        } else {
            tracing::debug!(error = %self, "request rejected");
        }

        (
            status,
            Json(ApiErrorBody {
                error: ApiErrorDetail { code, message: msg },
            }),
        )
            .into_response()
    }
}

pub type AppResult<T> = std::result::Result<T, AppError>;
