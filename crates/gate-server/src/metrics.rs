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
//! | `gateway_stage_duration_seconds` | Histogram | stage, outcome                     |
//! | `provider_route_decisions_total` | Counter   | provider_type, outcome             |
//! | `upstream_errors_total`          | Counter   | kind                               |
//! | `provider_runtime_snapshot_version` | Gauge  | (none)                             |
//! | `billing_outbox_lag_seconds`     | Gauge     | (none)                             |
//! | `billing_settle_failures_total`  | Counter   | reason                             |
//! | `usage_rollup_lag_seconds`       | Gauge     | (none)                             |
//! | `worker_lease_owner`             | Gauge     | job                                |

use axum::http::StatusCode;
use axum::response::IntoResponse;
use gate_core::id::ChannelId;
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

/// Record one gateway pipeline stage with bounded-cardinality labels.
pub fn record_gateway_stage(stage: &'static str, outcome: &'static str, duration_secs: f64) {
    metrics::histogram!(
        "gateway_stage_duration_seconds",
        "stage" => stage,
        "outcome" => outcome,
    )
    .record(duration_secs);
}

/// Record provider routing decisions without high-cardinality channel/model labels.
pub fn record_provider_route_decision(
    provider_type: &str,
    outcome: &'static str,
    snapshot_version: u64,
    channel_id: Option<ChannelId>,
) {
    metrics::counter!(
        "provider_route_decisions_total",
        "provider_type" => provider_type.to_string(),
        "outcome" => outcome,
    )
    .increment(1);
    metrics::gauge!("provider_runtime_snapshot_version").set(snapshot_version as f64);
    if let Some(channel_id) = channel_id {
        tracing::debug!(
            channel_id = %channel_id.as_uuid(),
            provider_type,
            outcome,
            snapshot_version,
            "provider route decision"
        );
    }
}

/// Read model freshness signal for usage rollups.
pub fn record_usage_rollup_lag_seconds(lag_seconds: f64) {
    metrics::gauge!("usage_rollup_lag_seconds").set(lag_seconds);
}

/// Billing settlement failure signal.
pub fn record_billing_settle_failure(reason: &'static str) {
    metrics::counter!("billing_settle_failures_total", "reason" => reason).increment(1);
}

/// Record normalized upstream/provider errors with bounded labels.
pub fn record_upstream_error(kind: &'static str) {
    metrics::counter!("upstream_errors_total", "kind" => kind).increment(1);
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
    (len == 32 || len == 36) && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
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
        let path =
            "/v1/orgs/550e8400e29b41d4a716446655440000/projects/660e8400e29b41d4a716446655440001";
        assert_eq!(normalize_path(path), "/v1/orgs/:id/projects/:id");
    }

    #[test]
    fn uuid_detection() {
        assert!(looks_like_uuid("550e8400-e29b-41d4-a716-446655440000")); // 36 chars dashed
        assert!(looks_like_uuid("550e8400e29b41d4a716446655440000")); // 32 chars simple
        assert!(!looks_like_uuid("projects"));
        assert!(!looks_like_uuid("me"));
    }
}
