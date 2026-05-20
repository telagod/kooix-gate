//! Provider error normalization for OpenAI-compatible API responses and health policy.

use axum::http::StatusCode;
use gate_providers::error::NormalizedProviderErrorKind;

pub(crate) struct ProviderApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub error_type: &'static str,
    pub message: String,
    pub retry_after_ms: Option<u64>,
    pub upstream_status: Option<u16>,
    pub upstream_code: Option<String>,
}

impl ProviderApiError {
    pub(crate) fn from_error(error: &gate_providers::ProviderError) -> Self {
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
        crate::audit_redaction::redact_text(&trimmed.chars().take(512).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gate_providers::ProviderError;

    #[test]
    fn provider_error_message_redacts_api_keys_and_bearer_tokens() {
        let normalized = ProviderApiError::from_error(&ProviderError::InvalidRequest(
            "upstream echoed Authorization: Bearer sk-proj-live-secret and api_key=sk-test-secret"
                .into(),
        ));

        assert!(normalized.message.contains("[REDACTED]"));
        assert!(!normalized.message.contains("sk-proj-live-secret"));
        assert!(!normalized.message.contains("sk-test-secret"));
    }
}
