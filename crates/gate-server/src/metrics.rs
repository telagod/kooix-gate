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
//! | `gateway_requests_total`         | Counter   | method, path, status, status_class |
//! | `gateway_request_duration_seconds` | Histogram | method, path, status_class       |
//! | `gate_tokens_total`              | Counter   | type (prompt/completion), model     |
//! | `gate_active_requests`           | Gauge     | (none)                             |
//! | `gateway_stage_duration_seconds` | Histogram | stage, outcome                     |
//! | `provider_route_decisions_total` | Counter   | provider_type, outcome             |
//! | `provider_health_probe_total`   | Counter   | provider_type, outcome, status_bucket |
//! | `provider_health_probe_duration_seconds` | Histogram | provider_type, outcome, status_bucket |
//! | `upstream_errors_total`          | Counter   | kind                               |
//! | `gateway_upstream_errors_total`  | Counter   | kind, provider_type, channel, model |
//! | `quota_denies_total`             | Counter   | dimension, scope_kind, mode        |
//! | `provider_runtime_snapshot_version` | Gauge  | (none)                             |
//! | `billing_outbox_lag_seconds`     | Gauge     | (none)                             |
//! | `billing_settle_lag_seconds`     | Gauge     | (none)                             |
//! | `billing_settle_failures_total`  | Counter   | reason                             |
//! | `usage_rollup_lag_seconds`       | Gauge     | (none)                             |
//! | `worker_lease_owner`             | Gauge     | job                                |

use axum::http::StatusCode;
use axum::response::IntoResponse;
use gate_core::id::ChannelId;
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;
use uuid::Uuid;

const UNKNOWN_LABEL: &str = "unknown";
const FALLBACK_CHANNEL_LABEL: &str = "fallback";
const MAX_LABEL_CHARS: usize = 96;
const REQUEST_DURATION_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Global handle to the Prometheus recorder. Initialized exactly once by
/// [`install_recorder`]. The handle is used by the `/metrics` endpoint to
/// render the exposition format.
static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the Prometheus metrics recorder (global, once).
///
/// Must be called early in startup (before any `metrics::counter!` calls).
/// Returns `true` if installation succeeded, `false` if already installed.
pub fn install_recorder() -> bool {
    let builder = PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("gate_request_duration_seconds".to_string()),
            REQUEST_DURATION_BUCKETS,
        )
        .expect("request duration histogram buckets are non-empty")
        .set_buckets_for_metric(
            Matcher::Full("gateway_request_duration_seconds".to_string()),
            REQUEST_DURATION_BUCKETS,
        )
        .expect("gateway request duration histogram buckets are non-empty")
        .set_buckets_for_metric(
            Matcher::Full("gateway_stage_duration_seconds".to_string()),
            REQUEST_DURATION_BUCKETS,
        )
        .expect("gateway stage duration histogram buckets are non-empty")
        .set_buckets_for_metric(
            Matcher::Full("provider_health_probe_duration_seconds".to_string()),
            REQUEST_DURATION_BUCKETS,
        )
        .expect("provider health duration histogram buckets are non-empty");

    match builder.install_recorder() {
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

pub fn render_for_tests() -> Option<String> {
    HANDLE.get().map(PrometheusHandle::render)
}

/// Record a completed HTTP request (called by the metrics middleware).
pub fn record_request(method: &str, path: &str, status: u16, duration_secs: f64) {
    let status_str = status.to_string();
    let status_class = status_class(status);
    let method = method.to_ascii_uppercase();
    let path_normalized = normalize_path(path);

    metrics::counter!(
        "gate_requests_total",
        "method" => method.clone(),
        "path" => path_normalized.clone(),
        "status" => status_str.clone(),
    )
    .increment(1);
    metrics::counter!(
        "gateway_requests_total",
        "method" => method.clone(),
        "path" => path_normalized.clone(),
        "status" => status_str,
        "status_class" => status_class,
    )
    .increment(1);

    metrics::histogram!(
        "gate_request_duration_seconds",
        "method" => method.clone(),
        "path" => path_normalized.clone(),
    )
    .record(duration_secs);
    metrics::histogram!(
        "gateway_request_duration_seconds",
        "method" => method,
        "path" => path_normalized,
        "status_class" => status_class,
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
    metrics::gauge!("usage_rollup_lag_seconds").set(lag_seconds.max(0.0));
}

/// Billing outbox backlog age from event occurrence to enqueue / fetch.
pub fn record_billing_outbox_lag_seconds(lag_seconds: f64) {
    metrics::gauge!("billing_outbox_lag_seconds").set(lag_seconds.max(0.0));
}

/// Billing settlement failure signal.
pub fn record_billing_settle_failure(reason: &'static str) {
    metrics::counter!("billing_settle_failures_total", "reason" => reason).increment(1);
}

/// Billing settlement age from request occurrence to committed usage projection.
pub fn record_billing_settle_lag_seconds(lag_seconds: f64) {
    metrics::gauge!("billing_settle_lag_seconds").set(lag_seconds.max(0.0));
}

/// Record quota hard denies with bounded labels.
pub fn record_quota_deny(dimension: &str, scope_kind: &str, mode: &str) {
    metrics::counter!(
        "quota_denies_total",
        "dimension" => normalize_label_value(dimension),
        "scope_kind" => normalize_label_value(scope_kind),
        "mode" => normalize_label_value(mode),
    )
    .increment(1);
}

/// Record normalized upstream/provider errors with legacy kind-only compatibility.
/// Record normalized upstream/provider errors by provider/channel/model.
///
/// `channel` should be a typed channel ID (`ch_...`), `fallback`, or `unrouted`.
/// Model and provider labels are sanitized and truncated to avoid cardinality
/// explosions from arbitrary upstream / user-controlled strings.
pub fn record_upstream_error_with_context(
    kind: &'static str,
    provider_type: &str,
    channel: &str,
    model: &str,
) {
    metrics::counter!("upstream_errors_total", "kind" => kind).increment(1);
    metrics::counter!(
        "gateway_upstream_errors_total",
        "kind" => kind,
        "provider_type" => normalize_label_value(provider_type),
        "channel" => normalize_label_value(channel),
        "model" => normalize_label_value(model),
    )
    .increment(1);
}

pub fn channel_label(channel_id: Option<Uuid>) -> String {
    channel_id
        .map(|id| ChannelId::from(id).to_string())
        .unwrap_or_else(|| FALLBACK_CHANNEL_LABEL.to_string())
}

/// Record one background health probe with bounded-cardinality labels.
pub fn record_health_probe(
    provider_type: &str,
    outcome: &'static str,
    status_bucket: &'static str,
    duration_secs: f64,
) {
    metrics::counter!(
        "provider_health_probe_total",
        "provider_type" => provider_type.to_string(),
        "outcome" => outcome,
        "status_bucket" => status_bucket,
    )
    .increment(1);
    metrics::histogram!(
        "provider_health_probe_duration_seconds",
        "provider_type" => provider_type.to_string(),
        "outcome" => outcome,
        "status_bucket" => status_bucket,
    )
    .record(duration_secs);
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

fn status_class(status: u16) -> &'static str {
    match status {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "other",
    }
}

fn normalize_label_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return UNKNOWN_LABEL.to_string();
    }

    let mut out = String::with_capacity(trimmed.len().min(MAX_LABEL_CHARS));
    for ch in trimmed.chars().take(MAX_LABEL_CHARS) {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }

    if out.is_empty() {
        UNKNOWN_LABEL.to_string()
    } else {
        out
    }
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

    #[test]
    fn status_class_is_stable() {
        assert_eq!(status_class(200), "2xx");
        assert_eq!(status_class(429), "4xx");
        assert_eq!(status_class(502), "5xx");
        assert_eq!(status_class(799), "other");
    }

    #[test]
    fn label_values_are_bounded_and_sanitized() {
        assert_eq!(normalize_label_value("  gpt-4o/mini "), "gpt-4o/mini");
        assert_eq!(normalize_label_value(""), "unknown");
        assert_eq!(
            normalize_label_value("bad label\twith space"),
            "bad_label_with_space"
        );
        assert!(normalize_label_value(&"x".repeat(200)).len() <= MAX_LABEL_CHARS);
    }
}
