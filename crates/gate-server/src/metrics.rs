//! Prometheus metrics endpoint + application metric definitions.
//!
//! Uses the `metrics` facade crate with the `metrics-exporter-prometheus`
//! backend. Metrics are recorded throughout the codebase via `metrics::counter!`,
//! `metrics::histogram!`, etc., and rendered as Prometheus exposition format
//! at `GET /metrics`.
//!
//! ## Defined metrics
//!
//! | Name                             | Type      | Labels                             |
//! |----------------------------------|-----------|------------------------------------|
//! | `gate_requests_total`            | Counter   | method, path, status               |
//! | `gate_request_duration_seconds`  | Histogram | method, path                       |
//! | `gate_tokens_total`              | Counter   | type (prompt/completion), model     |
//! | `gate_active_requests`           | Gauge     | (none)                             |

use axum::http::StatusCode;
use axum::response::IntoResponse;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;

/// Global handle to the Prometheus recorder. Initialized exactly once by
/// [`install_recorder`]. The handle is used by the `/metrics` endpoint to
/// render the exposition format.
static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the Prometheus metrics recorder (global, once).
///
/// Must be called early in startup (before any `metrics::counter!` calls).
/// Returns `true` if installation succeeded, `false` if already installed.
pub fn install_recorder() -> bool {
    match PrometheusBuilder::new().install_recorder() {
        Ok(handle) => {
            HANDLE.get_or_init(|| handle);
            true
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to install prometheus recorder (may already be installed)");
            false
        }
    }
}

/// `GET /metrics` handler — returns Prometheus exposition format.
pub async fn metrics_handler() -> impl IntoResponse {
    match HANDLE.get() {
        Some(handle) => {
            let body = handle.render();
            (
                StatusCode::OK,
                [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
                body,
            )
                .into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "metrics recorder not initialized",
        )
            .into_response(),
    }
}

/// Record a completed HTTP request (called by the metrics middleware).
pub fn record_request(method: &str, path: &str, status: u16, duration_secs: f64) {
    let status_str = status.to_string();
    let path_normalized = normalize_path(path);

    metrics::counter!(
        "gate_requests_total",
        "method" => method.to_string(),
        "path" => path_normalized.clone(),
        "status" => status_str,
    )
    .increment(1);

    metrics::histogram!(
        "gate_request_duration_seconds",
        "method" => method.to_string(),
        "path" => path_normalized,
    )
    .record(duration_secs);
}

/// Record token usage from a chat completion.
pub fn record_tokens(model: &str, prompt: u64, completion: u64) {
    let model = model.to_string();
    metrics::counter!(
        "gate_tokens_total",
        "type" => "prompt",
        "model" => model.clone(),
    )
    .increment(prompt);
    metrics::counter!(
        "gate_tokens_total",
        "type" => "completion",
        "model" => model,
    )
    .increment(completion);
}

/// Normalize URL paths to avoid label cardinality explosion.
///
/// Replaces UUID-like segments with `:id`:
/// - `/v1/projects/550e8400-e29b-41d4-a716-446655440000/keys` → `/v1/projects/:id/keys`
fn normalize_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut segments = path.split('/');
    if let Some(first) = segments.next() {
        out.push_str(first);
    }
    for seg in segments {
        out.push('/');
        if looks_like_uuid(seg) {
            out.push_str(":id");
        } else {
            out.push_str(seg);
        }
    }
    out
}

/// Quick heuristic: 32-36 chars of hex + dashes.
fn looks_like_uuid(s: &str) -> bool {
    let len = s.len();
    (len == 32 || len == 36)
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_replaces_uuids() {
        let path = "/v1/projects/550e8400-e29b-41d4-a716-446655440000/keys";
        assert_eq!(normalize_path(path), "/v1/projects/:id/keys");
    }

    #[test]
    fn normalize_leaves_non_uuid_alone() {
        let path = "/v1/chat/completions";
        assert_eq!(normalize_path(path), "/v1/chat/completions");
    }

    #[test]
    fn normalize_multiple_uuids() {
        let path = "/v1/orgs/550e8400e29b41d4a716446655440000/projects/660e8400e29b41d4a716446655440001";
        assert_eq!(normalize_path(path), "/v1/orgs/:id/projects/:id");
    }

    #[test]
    fn uuid_detection() {
        assert!(looks_like_uuid("550e8400-e29b-41d4-a716-446655440000")); // 36 chars dashed
        assert!(looks_like_uuid("550e8400e29b41d4a716446655440000"));     // 32 chars simple
        assert!(!looks_like_uuid("projects"));
        assert!(!looks_like_uuid("me"));
    }
}
