//! Quota 执行 middleware：在 data-plane / control-plane 路径上实时拦截或 dry-run。
//!
//! 支持维度：
//! - `rpm` / `tpm`：Redis sliding window
//! - `concurrent`：Redis pre-debit 1 unit，request 完成后 refund
//! - `daily_budget_usd` / `monthly_budget_usd` / `lifetime_budget_usd`：费用 micros pre-debit
//! - `lifetime_tokens`：token units pre-debit，request 完成后按真实 usage settle
//!
//! 设计要点：
//! - 加载 quota 行的主体扇区（apikey: api_key + project + org；user: user + org）
//! - `model_filter` 在 middleware 里按请求 model 精确过滤；`*` 只做简单 glob
//! - `mode=dry_run` 只记录 `quota_dry_run_total` 与 tracing，不实际 debit / 不拦截
//! - 任意 enforce 维度超限 → 429 + `quota_exceeded` + `Retry-After`
//! - fail-open：Redis / DB 异常只 warn，不把整站卡死

use crate::cost_estimate::{DEFAULT_RATE_PER_TOKEN_MICROS, estimate_cost_micros};
use crate::inflight::{InflightGuard, InflightGuards, QuotaMetric};
use crate::middleware::KooixRequestId;
use crate::state::AppState;
use axum::Json;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use gate_auth::{AuthContext, Subject};
use gate_cache::{QuotaCounter, RateLimiter};
use gate_providers::{
    AudioSpeechRequest, ChatRequest, EmbeddingInput, EmbeddingRequest, ImageGenerationRequest,
};
use gate_storage::QuotaRecord;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;

const DAILY_TTL_SECONDS: i64 = 86_400;
const MONTHLY_TTL_SECONDS: i64 = 30 * DAILY_TTL_SECONDS;
const LIFETIME_TTL_SECONDS: i64 = 0;
const CONCURRENT_TTL_SECONDS: i64 = 5 * 60;

/// 主体挂载点 — (scope_kind, scope_id) 元组。
struct ScopeRef {
    kind: &'static str,
    id: uuid::Uuid,
}

/// 请求体估算结果。费用和 token 分开，避免 lifetime_tokens 被 cost settle 污染。
#[derive(Debug, Clone, Default)]
struct RequestEstimate {
    cost_micros: i64,
    tokens: i64,
    model: Option<String>,
}

/// 解析 AuthContext → quota 加载的扇区列表。
///
/// - ApiKey 主体：扇区 = api_key + project + org
/// - User 主体：扇区 = user + current_org（current_org 缺失则只查 user）
/// - 其他（anonymous / System）：返回空 → 放行
fn scopes_for(ctx: &AuthContext) -> Vec<ScopeRef> {
    let mut out = Vec::with_capacity(3);
    match ctx.subject() {
        Some(Subject::ApiKey {
            api_key_id,
            project_id,
            org_id,
        }) => {
            out.push(ScopeRef {
                kind: "api_key",
                id: *api_key_id.as_uuid(),
            });
            out.push(ScopeRef {
                kind: "project",
                id: *project_id.as_uuid(),
            });
            out.push(ScopeRef {
                kind: "org",
                id: *org_id.as_uuid(),
            });
        }
        Some(Subject::User { user_id, .. }) => {
            out.push(ScopeRef {
                kind: "user",
                id: *user_id.as_uuid(),
            });
            if let Some(org) = ctx.current_org() {
                out.push(ScopeRef {
                    kind: "org",
                    id: *org.as_uuid(),
                });
            }
        }
        _ => {}
    }
    out
}

/// 用 quota 行构造 Redis key —— 把 scope_kind/scope_id/dimension/model_filter/quota_id
/// 全部纳入 key 命名，避免不同 quota 串扰。
fn rate_key(q: &QuotaRecord) -> String {
    let mf = q.model_filter.as_deref().unwrap_or("*");
    format!(
        "qt:{}:{}:{}:{}:{}",
        q.scope_kind, q.scope_id, q.dimension, mf, q.id
    )
}

/// 计数器 key：budget / lifetime / concurrent 各自独立 prefix。
pub(crate) fn quota_counter_key(q: &QuotaRecord) -> String {
    let mf = q.model_filter.as_deref().unwrap_or("*");
    let prefix = match q.dimension.as_str() {
        "monthly_budget_usd" => "qb:m",
        "lifetime_budget_usd" => "qb:l",
        "lifetime_tokens" => "qtok:l",
        "concurrent" => "qc",
        _ => "qb:d",
    };
    format!(
        "{}:{}:{}:{}:{}:{}",
        prefix, q.scope_kind, q.scope_id, q.dimension, mf, q.id
    )
}

pub(crate) fn quota_ttl_seconds(dimension: &str) -> i64 {
    match dimension {
        "monthly_budget_usd" => MONTHLY_TTL_SECONDS,
        "lifetime_budget_usd" | "lifetime_tokens" => LIFETIME_TTL_SECONDS,
        "concurrent" => CONCURRENT_TTL_SECONDS,
        _ => DAILY_TTL_SECONDS,
    }
}

/// quota 检查决策。Allowed 透传；Denied 直接走 429。
enum Decision {
    Allowed,
    Denied {
        dimension: String,
        retry_after_ms: u64,
    },
}

fn quota_mode(q: &QuotaRecord) -> &str {
    if q.mode == "dry_run" {
        "dry_run"
    } else {
        "enforce"
    }
}

fn is_dry_run(q: &QuotaRecord) -> bool {
    quota_mode(q) == "dry_run"
}

fn record_dry_run(q: &QuotaRecord, would_deny: bool, current_used: i64, limit: i64) {
    metrics::counter!(
        "quota_dry_run_total",
        "dimension" => q.dimension.clone(),
        "scope_kind" => q.scope_kind.clone(),
        "would_deny" => would_deny.to_string(),
    )
    .increment(1);
    tracing::info!(
        quota_id = %q.id,
        scope_kind = %q.scope_kind,
        scope_id = %q.scope_id,
        dimension = %q.dimension,
        model_filter = q.model_filter.as_deref().unwrap_or("*"),
        current_used,
        limit,
        would_deny,
        "quota dry-run evaluated"
    );
}

fn model_matches(filter: Option<&str>, model: Option<&str>) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    if filter == "*" {
        return true;
    }
    let Some(model) = model else {
        return false;
    };
    if !filter.contains('*') {
        return filter == model;
    }
    let parts: Vec<&str> = filter.split('*').collect();
    let mut rest = model;
    let mut first = true;
    for part in parts.iter().copied().filter(|p| !p.is_empty()) {
        if first && !filter.starts_with('*') {
            if let Some(stripped) = rest.strip_prefix(part) {
                rest = stripped;
            } else {
                return false;
            }
        } else if let Some(idx) = rest.find(part) {
            rest = &rest[idx + part.len()..];
        } else {
            return false;
        }
        first = false;
    }
    filter.ends_with('*') || parts.last().is_none_or(|last| model.ends_with(last))
}

fn limit_as_i64(q: &QuotaRecord, unit: &str) -> Option<i64> {
    let value = if unit == "micros" {
        q.limit_value * Decimal::from(1_000_000)
    } else {
        q.limit_value
    };
    value.to_i64().filter(|n| *n >= 0)
}

/// 检查单条 rate 类 quota（rpm/tpm）。失败一律 fail-open。
async fn check_rate(limiter: &Arc<RateLimiter>, q: &QuotaRecord, amount: i64) -> Decision {
    let limit = match limit_as_i64(q, "units") {
        Some(n) => n,
        None => {
            tracing::warn!(quota_id = %q.id, "rate quota has non-i64 limit_value; fail-open");
            return Decision::Allowed;
        }
    };
    let window_secs = q.window_seconds.unwrap_or(60).max(1) as u64;
    let key = rate_key(q);

    if is_dry_run(q) {
        let current = match limiter.peek_count(&key, window_secs * 1000).await {
            Ok(n) => n as i64,
            Err(e) => {
                tracing::warn!(error = %e, quota_id = %q.id, "rate quota dry-run peek failed; fail-open");
                0
            }
        };
        record_dry_run(q, current.saturating_add(amount) > limit, current, limit);
        return Decision::Allowed;
    }

    match limiter
        .check_n(&key, window_secs * 1000, limit as u64, amount.max(0) as u64)
        .await
    {
        Ok(d) if d.allowed => Decision::Allowed,
        Ok(d) => Decision::Denied {
            dimension: q.dimension.clone(),
            retry_after_ms: d.retry_after_ms,
        },
        Err(e) => {
            tracing::warn!(error = %e, quota_id = %q.id, "rate quota check failed; fail-open");
            Decision::Allowed
        }
    }
}

async fn check_counter_predebit(
    qc: &Arc<QuotaCounter>,
    q: &QuotaRecord,
    amount: i64,
    limit: i64,
    metric: QuotaMetric,
) -> (Decision, Option<InflightGuard>) {
    let key = quota_counter_key(q);
    let ttl = quota_ttl_seconds(&q.dimension);

    if is_dry_run(q) {
        let current = match qc.peek(&key).await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %e, quota_id = %q.id, "quota dry-run peek failed; fail-open");
                0
            }
        };
        record_dry_run(q, current.saturating_add(amount) > limit, current, limit);
        return (Decision::Allowed, None);
    }

    match qc.debit(&key, amount, limit, ttl).await {
        Ok(outcome) if outcome.ok => {
            let guard = InflightGuard::new(qc.clone(), key, amount, metric);
            (Decision::Allowed, Some(guard))
        }
        Ok(_) => (
            Decision::Denied {
                dimension: q.dimension.clone(),
                retry_after_ms: if q.dimension == "concurrent" {
                    30_000
                } else {
                    60 * 60 * 1000
                },
            },
            None,
        ),
        Err(e) => {
            tracing::warn!(error = %e, quota_id = %q.id, "quota counter debit failed; fail-open");
            (Decision::Allowed, None)
        }
    }
}

fn has_body_metered_quota(q: &QuotaRecord) -> bool {
    matches!(
        q.dimension.as_str(),
        "tpm"
            | "daily_budget_usd"
            | "monthly_budget_usd"
            | "lifetime_budget_usd"
            | "lifetime_tokens"
    )
}

/// quota_enforce middleware 主入口。
pub async fn quota_enforce(State(state): State<AppState>, req: Request, next: Next) -> Response {
    // middleware 比 extractor 早执行，extension 里通常还没塞 AuthContext，
    // 这里主动解析一次（resolve 内部走 InMemoryLoader / PgLoader）。
    // 解析失败（坏 token、撤销的 key 等）让后续 extractor 去抛 401/403，
    // 这里走 fail-open 不卡死请求。
    let (mut parts, body) = req.into_parts();
    let ctx = match crate::auth::resolve_for_state(&mut parts, &state).await {
        Ok(c) => c,
        Err(_) => {
            let req = Request::from_parts(parts, body);
            return next.run(req).await;
        }
    };

    if ctx.subject().is_none() {
        let req = Request::from_parts(parts, body);
        return next.run(req).await;
    }

    let scopes = scopes_for(&ctx);
    if scopes.is_empty() {
        let req = Request::from_parts(parts, body);
        return next.run(req).await;
    }

    let mut quotas: Vec<QuotaRecord> = Vec::new();
    for s in &scopes {
        match state.repos.quotas.find_active_for(s.kind, s.id).await {
            Ok(mut rows) => quotas.append(&mut rows),
            Err(e) => {
                tracing::warn!(error = %e, scope = %s.kind, id = %s.id, "quota load failed; fail-open");
            }
        }
    }
    if quotas.is_empty() {
        let req = Request::from_parts(parts, body);
        return next.run(req).await;
    }

    let needs_body = quotas.iter().any(|q| {
        has_body_metered_quota(q)
            || q.model_filter
                .as_deref()
                .map(str::trim)
                .is_some_and(|filter| !filter.is_empty() && filter != "*")
    });
    let (estimate, body) = if needs_body {
        let bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
            Ok(b) => b,
            Err(_) => {
                let req = Request::from_parts(parts, Body::empty());
                return next.run(req).await;
            }
        };
        let est = estimate_data_plane_request(&bytes);
        (est, Body::from(bytes))
    } else {
        (RequestEstimate::default(), body)
    };

    quotas.retain(|q| model_matches(q.model_filter.as_deref(), estimate.model.as_deref()));
    if quotas.is_empty() {
        let req = Request::from_parts(parts, body);
        return next.run(req).await;
    }

    let mut guards: Vec<InflightGuard> = Vec::new();
    for q in &quotas {
        let decision = match q.dimension.as_str() {
            "rpm" => match &state.rate_limiter {
                Some(rl) => check_rate(rl, q, 1).await,
                None => Decision::Allowed,
            },
            "tpm" => match &state.rate_limiter {
                Some(rl) => check_rate(rl, q, estimate.tokens.max(1)).await,
                None => Decision::Allowed,
            },
            "concurrent" => match &state.quota_counter {
                Some(qc) => {
                    let limit = match limit_as_i64(q, "units") {
                        Some(n) => n,
                        None => {
                            tracing::warn!(quota_id = %q.id, "concurrent quota limit invalid; fail-open");
                            continue;
                        }
                    };
                    let (d, guard) =
                        check_counter_predebit(qc, q, 1, limit, QuotaMetric::Concurrent).await;
                    if let Some(g) = guard {
                        guards.push(g);
                    }
                    d
                }
                None => Decision::Allowed,
            },
            "daily_budget_usd" | "monthly_budget_usd" | "lifetime_budget_usd" => {
                match &state.quota_counter {
                    Some(qc) => {
                        let limit = match limit_as_i64(q, "micros") {
                            Some(n) => n,
                            None => {
                                tracing::warn!(quota_id = %q.id, "budget quota limit invalid; fail-open");
                                continue;
                            }
                        };
                        let (d, guard) = check_counter_predebit(
                            qc,
                            q,
                            estimate.cost_micros.max(0),
                            limit,
                            QuotaMetric::CostMicros,
                        )
                        .await;
                        if let Some(g) = guard {
                            guards.push(g);
                        }
                        d
                    }
                    None => Decision::Allowed,
                }
            }
            "lifetime_tokens" => match &state.quota_counter {
                Some(qc) => {
                    let limit = match limit_as_i64(q, "units") {
                        Some(n) => n,
                        None => {
                            tracing::warn!(quota_id = %q.id, "lifetime_tokens quota limit invalid; fail-open");
                            continue;
                        }
                    };
                    let (d, guard) = check_counter_predebit(
                        qc,
                        q,
                        estimate.tokens.max(1),
                        limit,
                        QuotaMetric::Tokens,
                    )
                    .await;
                    if let Some(g) = guard {
                        guards.push(g);
                    }
                    d
                }
                None => Decision::Allowed,
            },
            _ => Decision::Allowed,
        };
        if let Decision::Denied {
            dimension,
            retry_after_ms,
        } = decision
        {
            // 被拒绝时，需要退还已成功的 guard（本请求不会走到 handler）
            drop(guards);
            return quota_exceeded_response(&dimension, retry_after_ms).into_response();
        }
    }

    if !guards.is_empty() {
        let request_id = parts
            .extensions
            .get::<KooixRequestId>()
            .map(|id| id.0)
            .unwrap_or_else(uuid::Uuid::now_v7);
        let quota_keys: Vec<String> = guards.iter().map(|g| g.key.clone()).collect();
        let reserved_units: Vec<i64> = guards.iter().map(|g| g.reserved_units).collect();

        let (proj_id, key_id) = match ctx.subject() {
            Some(gate_auth::context::Subject::ApiKey {
                project_id,
                api_key_id,
                ..
            }) => (Some(*project_id.as_uuid()), Some(*api_key_id.as_uuid())),
            _ => (None, None),
        };

        let inflight_repo = state.repos.inflight.clone();
        let record = gate_storage::InFlightRecord {
            request_id,
            project_id: proj_id.unwrap_or_default(),
            api_key_id: key_id.unwrap_or_default(),
            channel_id: None,
            model: estimate.model.clone().unwrap_or_default(),
            estimated_cost_usd: estimate.cost_micros as f64 / 1_000_000.0,
            estimated_tokens: estimate.tokens.min(i32::MAX as i64).max(0) as i32,
            started_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
            quota_keys: quota_keys.clone(),
            estimated_micros: reserved_units.clone(),
        };
        if let Err(e) = inflight_repo.insert(&record).await {
            tracing::warn!(error = %e, "inflight insert failed (crash recovery degraded)");
        }

        let guards: Vec<_> = guards
            .into_iter()
            .map(|g| g.with_db(request_id, inflight_repo.clone()))
            .collect();

        parts.extensions.insert(InflightGuards::new(guards));
    }

    parts.extensions.insert(ctx);
    let req = Request::from_parts(parts, body);
    next.run(req).await
}

#[derive(serde::Serialize)]
struct ErrBody<'a> {
    error: ErrDetail<'a>,
}

#[derive(serde::Serialize)]
struct ErrDetail<'a> {
    code: &'a str,
    r#type: &'a str,
    message: String,
    dimension: &'a str,
    retry_after_ms: u64,
}

fn quota_exceeded_response(dimension: &str, retry_after_ms: u64) -> impl IntoResponse + use<> {
    let secs = retry_after_ms.div_ceil(1000).max(1);
    let body = Json(ErrBody {
        error: ErrDetail {
            code: "quota_exceeded",
            r#type: "quota_error",
            message: format!("quota '{dimension}' exhausted"),
            dimension,
            retry_after_ms,
        },
    });
    let mut resp = (StatusCode::TOO_MANY_REQUESTS, body).into_response();
    resp.headers_mut().insert(
        "retry-after",
        HeaderValue::from_str(&secs.to_string()).unwrap_or(HeaderValue::from_static("1")),
    );
    resp
}

fn estimate_data_plane_request(bytes: &[u8]) -> RequestEstimate {
    if let Ok(chat_req) = serde_json::from_slice::<ChatRequest>(bytes) {
        let cost = estimate_cost_micros(&chat_req, DEFAULT_RATE_PER_TOKEN_MICROS);
        return RequestEstimate {
            cost_micros: cost,
            tokens: estimate_chat_tokens(&chat_req),
            model: Some(chat_req.model),
        };
    }
    if let Ok(audio_req) = serde_json::from_slice::<AudioSpeechRequest>(bytes) {
        return RequestEstimate {
            cost_micros: estimate_audio_speech_cost_micros(&audio_req),
            tokens: 0,
            model: Some(audio_req.model),
        };
    }
    if let Ok(embed_req) = serde_json::from_slice::<EmbeddingRequest>(bytes) {
        let prompt_tokens = estimate_embedding_tokens(&embed_req);
        return RequestEstimate {
            cost_micros: (prompt_tokens * DEFAULT_RATE_PER_TOKEN_MICROS)
                .min(crate::cost_estimate::MAX_ESTIMATE_MICROS),
            tokens: prompt_tokens,
            model: Some(embed_req.model),
        };
    }
    if let Ok(image_req) = serde_json::from_slice::<ImageGenerationRequest>(bytes) {
        return RequestEstimate {
            cost_micros: estimate_image_cost_micros(&image_req),
            tokens: 0,
            model: Some(image_req.model),
        };
    }
    RequestEstimate {
        cost_micros: 4096 * DEFAULT_RATE_PER_TOKEN_MICROS,
        tokens: 4096,
        model: None,
    }
}

#[cfg(test)]
fn estimate_data_plane_cost_micros(bytes: &[u8]) -> i64 {
    estimate_data_plane_request(bytes).cost_micros
}

fn estimate_chat_tokens(req: &ChatRequest) -> i64 {
    let prompt_tokens: i64 = req
        .messages
        .iter()
        .map(|m| (m.content_text().len() / 4) as i64)
        .sum();
    let completion_tokens = req.max_tokens.unwrap_or(1024) as i64;
    prompt_tokens.saturating_add(completion_tokens)
}

fn estimate_embedding_tokens(req: &EmbeddingRequest) -> i64 {
    let prompt_chars: usize = match &req.input {
        EmbeddingInput::Single(s) => s.len(),
        EmbeddingInput::Multiple(values) => values.iter().map(String::len).sum(),
    };
    (prompt_chars / 4) as i64
}

fn estimate_image_cost_micros(req: &ImageGenerationRequest) -> i64 {
    const DEFAULT_RATE_PER_IMAGE_MICROS: i64 = 80_000;
    let images = req.n.unwrap_or(1).max(1) as i64;
    (images * DEFAULT_RATE_PER_IMAGE_MICROS).min(crate::cost_estimate::MAX_ESTIMATE_MICROS)
}

fn estimate_audio_speech_cost_micros(req: &AudioSpeechRequest) -> i64 {
    const DEFAULT_RATE_PER_TTS_CHAR_MICROS: i64 = 1;
    let chars = req.input.chars().count() as i64;
    (chars * DEFAULT_RATE_PER_TTS_CHAR_MICROS).min(crate::cost_estimate::MAX_ESTIMATE_MICROS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_plane_estimator_reads_embedding_single_input() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "model": "text-embedding-3-small",
            "input": "abcdefghijkl"
        }))
        .unwrap();

        assert_eq!(
            estimate_data_plane_cost_micros(&bytes),
            3 * DEFAULT_RATE_PER_TOKEN_MICROS
        );
    }

    #[test]
    fn data_plane_estimator_reads_embedding_multiple_input() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "model": "text-embedding-3-small",
            "input": ["abcdefgh", "ijklmnop"]
        }))
        .unwrap();

        assert_eq!(
            estimate_data_plane_cost_micros(&bytes),
            4 * DEFAULT_RATE_PER_TOKEN_MICROS
        );
    }

    #[test]
    fn data_plane_estimator_reads_image_generation() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "model": "dall-e-3",
            "prompt": "draw a gate",
            "n": 2
        }))
        .unwrap();

        assert_eq!(estimate_data_plane_cost_micros(&bytes), 160_000);
    }

    #[test]
    fn data_plane_estimator_reads_audio_speech() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "model": "tts-1",
            "input": "邪修一念成音",
            "voice": "alloy"
        }))
        .unwrap();

        assert_eq!(estimate_data_plane_cost_micros(&bytes), 6);
    }

    #[test]
    fn data_plane_estimator_falls_back_for_unknown_body() {
        let bytes = br#"{\"foo\":\"bar\"}"#;

        assert_eq!(
            estimate_data_plane_cost_micros(bytes),
            4096 * DEFAULT_RATE_PER_TOKEN_MICROS
        );
    }

    #[test]
    fn model_filter_supports_exact_and_simple_glob() {
        assert!(model_matches(None, Some("gpt-4o")));
        assert!(model_matches(Some("gpt-4o"), Some("gpt-4o")));
        assert!(!model_matches(Some("gpt-4o"), Some("gpt-4o-mini")));
        assert!(model_matches(Some("gpt-4o*"), Some("gpt-4o-mini")));
        assert!(model_matches(Some("*-mini"), Some("gpt-4o-mini")));
        assert!(model_matches(Some("gpt-*mini"), Some("gpt-4o-mini")));
    }
}
