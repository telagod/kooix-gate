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
//! | `gate_chat_duration_seconds`     | Histogram | model, provider_type, streaming, outcome |
//! | `gate_chat_ttfb_seconds`         | Histogram | model, provider_type               |
//! | `gate_chat_stream_chunks_total`  | Counter   | model, provider_type, outcome      |
//! | `gate_chat_requests_total`       | Counter   | model, provider_type, streaming, outcome |

use axum::http::StatusCode;
use axum::response::IntoResponse;
use gate_core::id::ChannelId;
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::OnceLock;
use uuid::Uuid;

// ============================================================================
// 0.4.107（followup §3.5）：metric 名常量化
//
// 之前 metric 名字符串散在 metrics.rs 闭包内、observability.md 表格、Grafana
// dashboard JSON 三处。任何 typo（如把 `gate_chat_requests_total` 写成
// `gate_chat_request_total`）只能 PR review / 抓 bug 时发现。
//
// 抽 const + pub mod names 让外部代码（如未来 chat.rs 直接 metrics::counter!
// 调用、Grafana panel 生成器、observability.md 检查脚本）引用同一 const。
// ============================================================================
pub mod names {
    // Chat (0.4.66)
    pub const CHAT_REQUESTS_TOTAL: &str = "gate_chat_requests_total";
    pub const CHAT_DURATION_SECONDS: &str = "gate_chat_duration_seconds";
    pub const CHAT_TTFB_SECONDS: &str = "gate_chat_ttfb_seconds";
    pub const CHAT_STREAM_CHUNKS_TOTAL: &str = "gate_chat_stream_chunks_total";

    // HTTP lifecycle
    pub const REQUESTS_TOTAL: &str = "gate_requests_total";
    pub const GATEWAY_REQUESTS_TOTAL: &str = "gateway_requests_total";
    pub const REQUEST_DURATION_SECONDS: &str = "gate_request_duration_seconds";
    pub const GATEWAY_REQUEST_DURATION_SECONDS: &str = "gateway_request_duration_seconds";
    pub const TOKENS_TOTAL: &str = "gate_tokens_total";
    pub const ACTIVE_REQUESTS: &str = "gate_active_requests";
    pub const GATEWAY_STAGE_DURATION_SECONDS: &str = "gateway_stage_duration_seconds";

    // Upstream
    pub const UPSTREAM_ERRORS_TOTAL: &str = "upstream_errors_total";
    pub const GATEWAY_UPSTREAM_ERRORS_TOTAL: &str = "gateway_upstream_errors_total";

    // Provider routing / health
    pub const PROVIDER_ROUTE_DECISIONS_TOTAL: &str = "provider_route_decisions_total";
    pub const PROVIDER_HEALTH_PROBE_TOTAL: &str = "provider_health_probe_total";
    pub const PROVIDER_HEALTH_PROBE_DURATION_SECONDS: &str =
        "provider_health_probe_duration_seconds";
    pub const PROVIDER_RUNTIME_SNAPSHOT_VERSION: &str = "provider_runtime_snapshot_version";

    // Quota / billing
    pub const QUOTA_DENIES_TOTAL: &str = "quota_denies_total";
    pub const BILLING_OUTBOX_LAG_SECONDS: &str = "billing_outbox_lag_seconds";
    pub const BILLING_SETTLE_LAG_SECONDS: &str = "billing_settle_lag_seconds";
    pub const BILLING_SETTLE_FAILURES_TOTAL: &str = "billing_settle_failures_total";
    pub const USAGE_ROLLUP_LAG_SECONDS: &str = "usage_rollup_lag_seconds";

    // Worker
    pub const WORKER_LEASE_OWNER: &str = "worker_lease_owner";
}

const UNKNOWN_LABEL: &str = "unknown";
const FALLBACK_CHANNEL_LABEL: &str = "fallback";
const MAX_LABEL_CHARS: usize = 96;
const MAX_RUNTIME_SNAPSHOT_SERIES: usize = 2048;
const REQUEST_DURATION_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Global handle to the Prometheus recorder. Initialized exactly once by
/// [`install_recorder`]. The handle is used by the `/metrics` endpoint to
/// render the exposition format.
static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();
static QUOTA_DENY_SNAPSHOT: OnceLock<Mutex<HashMap<QuotaDenyKey, u64>>> = OnceLock::new();
static UPSTREAM_ERROR_SNAPSHOT: OnceLock<Mutex<HashMap<UpstreamErrorKey, u64>>> = OnceLock::new();

type QuotaDenyKey = (String, String, String);
type UpstreamErrorKey = (String, String, String, String);

#[derive(Debug, Clone, Serialize)]
pub struct QuotaDenySnapshot {
    pub dimension: String,
    pub scope_kind: String,
    pub mode: String,
    pub denies: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpstreamErrorSnapshot {
    pub kind: String,
    pub provider_type: String,
    pub channel: String,
    pub model: String,
    pub errors: u64,
}

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
        .expect("provider health duration histogram buckets are non-empty")
        .set_buckets_for_metric(
            Matcher::Full("gate_chat_duration_seconds".to_string()),
            REQUEST_DURATION_BUCKETS,
        )
        .expect("chat duration histogram buckets are non-empty")
        .set_buckets_for_metric(
            Matcher::Full("gate_chat_ttfb_seconds".to_string()),
            REQUEST_DURATION_BUCKETS,
        )
        .expect("chat ttfb histogram buckets are non-empty");

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

fn quota_deny_counters() -> &'static Mutex<HashMap<QuotaDenyKey, u64>> {
    QUOTA_DENY_SNAPSHOT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn upstream_error_counters() -> &'static Mutex<HashMap<UpstreamErrorKey, u64>> {
    UPSTREAM_ERROR_SNAPSHOT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn bump_quota_deny_snapshot(dimension: String, scope_kind: String, mode: String) {
    let mut counters = quota_deny_counters().lock();
    let key = if counters.len() < MAX_RUNTIME_SNAPSHOT_SERIES
        || counters.contains_key(&(dimension.clone(), scope_kind.clone(), mode.clone()))
    {
        (dimension, scope_kind, mode)
    } else {
        (
            "overflow".to_string(),
            "overflow".to_string(),
            "overflow".to_string(),
        )
    };
    *counters.entry(key).or_insert(0) += 1;
}

fn bump_upstream_error_snapshot(
    kind: String,
    provider_type: String,
    channel: String,
    model: String,
) {
    let mut counters = upstream_error_counters().lock();
    let key = if counters.len() < MAX_RUNTIME_SNAPSHOT_SERIES
        || counters.contains_key(&(
            kind.clone(),
            provider_type.clone(),
            channel.clone(),
            model.clone(),
        )) {
        (kind, provider_type, channel, model)
    } else {
        (
            "overflow".to_string(),
            "overflow".to_string(),
            "overflow".to_string(),
            "overflow".to_string(),
        )
    };
    *counters.entry(key).or_insert(0) += 1;
}

pub fn quota_deny_snapshot() -> Vec<QuotaDenySnapshot> {
    let mut rows: Vec<_> = quota_deny_counters()
        .lock()
        .iter()
        .map(
            |((dimension, scope_kind, mode), denies)| QuotaDenySnapshot {
                dimension: dimension.clone(),
                scope_kind: scope_kind.clone(),
                mode: mode.clone(),
                denies: *denies,
            },
        )
        .collect();
    rows.sort_by(|a, b| {
        b.denies
            .cmp(&a.denies)
            .then_with(|| a.dimension.cmp(&b.dimension))
            .then_with(|| a.scope_kind.cmp(&b.scope_kind))
            .then_with(|| a.mode.cmp(&b.mode))
    });
    rows
}

pub fn upstream_error_snapshot() -> Vec<UpstreamErrorSnapshot> {
    let mut rows: Vec<_> = upstream_error_counters()
        .lock()
        .iter()
        .map(
            |((kind, provider_type, channel, model), errors)| UpstreamErrorSnapshot {
                kind: kind.clone(),
                provider_type: provider_type.clone(),
                channel: channel.clone(),
                model: model.clone(),
                errors: *errors,
            },
        )
        .collect();
    rows.sort_by(|a, b| {
        b.errors
            .cmp(&a.errors)
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.provider_type.cmp(&b.provider_type))
            .then_with(|| a.channel.cmp(&b.channel))
            .then_with(|| a.model.cmp(&b.model))
    });
    rows
}

#[doc(hidden)]
pub fn reset_runtime_snapshots_for_tests() {
    quota_deny_counters().lock().clear();
    upstream_error_counters().lock().clear();
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
    metrics::gauge!(names::PROVIDER_RUNTIME_SNAPSHOT_VERSION).set(snapshot_version as f64);
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
    metrics::gauge!(names::USAGE_ROLLUP_LAG_SECONDS).set(lag_seconds.max(0.0));
}

/// Billing outbox backlog age from event occurrence to enqueue / fetch.
pub fn record_billing_outbox_lag_seconds(lag_seconds: f64) {
    metrics::gauge!(names::BILLING_OUTBOX_LAG_SECONDS).set(lag_seconds.max(0.0));
}

/// Billing settlement failure signal.
pub fn record_billing_settle_failure(reason: &'static str) {
    metrics::counter!(names::BILLING_SETTLE_FAILURES_TOTAL, "reason" => reason).increment(1);
}

/// Billing settlement age from request occurrence to committed usage projection.
pub fn record_billing_settle_lag_seconds(lag_seconds: f64) {
    metrics::gauge!(names::BILLING_SETTLE_LAG_SECONDS).set(lag_seconds.max(0.0));
}

/// Record quota hard denies with bounded labels.
pub fn record_quota_deny(dimension: &str, scope_kind: &str, mode: &str) {
    let dimension = normalize_label_value(dimension);
    let scope_kind = normalize_label_value(scope_kind);
    let mode = normalize_label_value(mode);
    metrics::counter!(
        "quota_denies_total",
        "dimension" => dimension.clone(),
        "scope_kind" => scope_kind.clone(),
        "mode" => mode.clone(),
    )
    .increment(1);
    bump_quota_deny_snapshot(dimension, scope_kind, mode);
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
    let provider_type = normalize_label_value(provider_type);
    let channel = normalize_label_value(channel);
    let model = normalize_label_value(model);
    metrics::counter!(names::UPSTREAM_ERRORS_TOTAL, "kind" => kind).increment(1);
    metrics::counter!(
        "gateway_upstream_errors_total",
        "kind" => kind,
        "provider_type" => provider_type.clone(),
        "channel" => channel.clone(),
        "model" => model.clone(),
    )
    .increment(1);
    bump_upstream_error_snapshot(kind.to_string(), provider_type, channel, model);
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

// ============================================================================
// Chat-specific metrics (0.4.66 — product-review A2)
//
// gate_chat_duration_seconds   — e2e chat handler latency (model, provider, streaming, outcome)
// gate_chat_ttfb_seconds       — first chunk latency for streaming chat (model, provider)
// gate_chat_stream_chunks_total — SSE chunk count per stream (model, provider, outcome)
// gate_chat_requests_total     — chat handler request count (model, provider, streaming, outcome)
//
// 标签卡死有限基数：
// - model：normalize_label_value 截 96 字符
// - provider_type：上游枚举固定（openai/anthropic/azure/bedrock/plugin/...）
// - streaming："true"/"false"
// - outcome："ok"/"error"
// ============================================================================

/// Record one chat handler invocation.
///
/// `outcome="ok"` 表示 handler 成功返回响应（非流式：JSON 200；流式：stream
/// 建立成功）。`outcome="error"` 表示 handler 异常出错（不区分上游错误 vs gate
/// 内部错误，前者另由 record_upstream_error 区分）。
pub fn record_chat_request(
    model: &str,
    provider_type: &str,
    streaming: bool,
    outcome: &'static str,
    duration_secs: f64,
) {
    let model = normalize_label_value(model);
    let provider_type = normalize_label_value(provider_type);
    let streaming_str = if streaming { "true" } else { "false" };
    metrics::counter!(
        names::CHAT_REQUESTS_TOTAL,
        "model" => model.clone(),
        "provider_type" => provider_type.clone(),
        "streaming" => streaming_str,
        "outcome" => outcome,
    )
    .increment(1);
    metrics::histogram!(
        names::CHAT_DURATION_SECONDS,
        "model" => model,
        "provider_type" => provider_type,
        "streaming" => streaming_str,
        "outcome" => outcome,
    )
    .record(duration_secs);
}

/// Record time-to-first-byte for streaming chat (i.e. first SSE chunk arrives).
pub fn record_chat_ttfb(model: &str, provider_type: &str, ttfb_secs: f64) {
    let model = normalize_label_value(model);
    let provider_type = normalize_label_value(provider_type);
    metrics::histogram!(
        names::CHAT_TTFB_SECONDS,
        "model" => model,
        "provider_type" => provider_type,
    )
    .record(ttfb_secs);
}

/// Record total SSE chunk count for one stream.
pub fn record_chat_stream_chunks(
    model: &str,
    provider_type: &str,
    outcome: &'static str,
    chunks: u64,
) {
    if chunks == 0 {
        return;
    }
    let model = normalize_label_value(model);
    let provider_type = normalize_label_value(provider_type);
    metrics::counter!(
        names::CHAT_STREAM_CHUNKS_TOTAL,
        "model" => model,
        "provider_type" => provider_type,
        "outcome" => outcome,
    )
    .increment(chunks);
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

    #[test]
    fn runtime_snapshots_capture_bounded_metrics() {
        reset_runtime_snapshots_for_tests();

        record_quota_deny("daily budget usd", "api key", "enforce");
        record_upstream_error_with_context(
            "authentication_error",
            "openai",
            "ch_test",
            "gpt-4o-mini",
        );

        let quota = quota_deny_snapshot();
        assert_eq!(quota.len(), 1);
        assert_eq!(quota[0].dimension, "daily_budget_usd");
        assert_eq!(quota[0].scope_kind, "api_key");
        assert_eq!(quota[0].denies, 1);

        let upstream = upstream_error_snapshot();
        assert_eq!(upstream.len(), 1);
        assert_eq!(upstream[0].kind, "authentication_error");
        assert_eq!(upstream[0].provider_type, "openai");
        assert_eq!(upstream[0].errors, 1);
    }

    #[test]
    fn chat_metrics_emit_through_recorder() {
        // Recorder may already be installed by another test in the same binary
        // — that's fine, we just need it usable.
        install_recorder();

        record_chat_request("gpt-4o-mini", "openai", true, "ok", 0.456);
        record_chat_ttfb("gpt-4o-mini", "openai", 0.087);
        record_chat_stream_chunks("gpt-4o-mini", "openai", "ok", 42);
        record_chat_request("claude-3-5-sonnet", "anthropic", false, "error", 1.234);

        let render = render_for_tests().expect("recorder renders prometheus output");
        assert!(
            render.contains("gate_chat_requests_total"),
            "expected gate_chat_requests_total in output:\n{render}"
        );
        assert!(
            render.contains("gate_chat_duration_seconds"),
            "expected gate_chat_duration_seconds in output"
        );
        assert!(
            render.contains("gate_chat_ttfb_seconds"),
            "expected gate_chat_ttfb_seconds in output"
        );
        assert!(
            render.contains("gate_chat_stream_chunks_total"),
            "expected gate_chat_stream_chunks_total in output"
        );
        // 验标签真的带上
        assert!(render.contains("provider_type=\"openai\""));
        assert!(render.contains("streaming=\"true\""));
        assert!(render.contains("outcome=\"ok\""));
    }
}
