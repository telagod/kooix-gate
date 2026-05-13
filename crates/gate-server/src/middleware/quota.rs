//! Quota 执行 middleware：在 /v1/chat/completions 等业务路径上实时拦截
//! rpm / tpm / daily_budget_usd / monthly_budget_usd 配额。
//!
//! 设计要点：
//! - 加载 quota 行的主体扇区（apikey: api_key + project + org；user: user + org）
//! - rpm/tpm 走 [`RateLimiter`](gate_cache::RateLimiter) 滑窗（window_seconds 来自 quota.window_seconds）
//! - budget 类只读判断（peek）：chat 请求事前不知道 cost，路径上不预扣；只过滤已超额
//! - 任意维度超限 → 429 + `quota_exceeded` + `Retry-After`
//! - fail-open：
//!   * RateLimiter 未配置 → 跳过 rate 维度
//!   * QuotaCounter 未配置 → 跳过 budget 维度
//!   * DB 查 quota 出错 → warn 并通过（避免把整站卡死）
//!
//! anonymous 主体直接放行（quota 只对真实身份生效）。
//!
//! 与 rate_limit middleware 的关系：rate_limit 是「单一全局桶」按 subject 分；
//! 这里是「显式配置的多维度配额」，两者叠加生效（rate_limit 在外层先拦）。

use crate::state::AppState;
use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use gate_auth::{AuthContext, Subject};
use gate_cache::RateLimiter;
use gate_storage::QuotaRecord;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;

/// 主体挂载点 — (scope_kind, scope_id) 元组。
struct ScopeRef {
    kind: &'static str,
    id: uuid::Uuid,
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

/// 用 quota 行构造 Redis key —— 把 scope_kind/scope_id/dimension/model_filter
/// 全部纳入 key 命名，避免不同 quota 串扰。
fn rate_key(q: &QuotaRecord) -> String {
    let mf = q.model_filter.as_deref().unwrap_or("*");
    format!(
        "qt:{}:{}:{}:{}:{}",
        q.scope_kind, q.scope_id, q.dimension, mf, q.id
    )
}

/// 与 rate_key 同源；budget 类计数器单独命名前缀避免和 rate 混。
fn budget_key(q: &QuotaRecord) -> String {
    let mf = q.model_filter.as_deref().unwrap_or("*");
    // 用 daily 前缀对应 24h TTL；月度 budget 走 monthly。
    let prefix = if q.dimension == "monthly_budget_usd" {
        "qb:m"
    } else {
        "qb:d"
    };
    format!(
        "{}:{}:{}:{}:{}:{}",
        prefix, q.scope_kind, q.scope_id, q.dimension, mf, q.id
    )
}

/// quota 检查决策。Allowed 透传；Denied 直接走 429。
enum Decision {
    Allowed,
    Denied {
        dimension: String,
        retry_after_ms: u64,
    },
}

/// 检查单条 rate 类 quota（rpm/tpm）。失败一律 fail-open。
async fn check_rate(limiter: &Arc<RateLimiter>, q: &QuotaRecord) -> Decision {
    let limit = match q.limit_value.to_i64() {
        Some(n) if n >= 0 => n as u64,
        _ => {
            tracing::warn!(
                quota_id = %q.id, "rate quota has non-i64 limit_value; fail-open"
            );
            return Decision::Allowed;
        }
    };
    let window_secs = q.window_seconds.unwrap_or(60).max(1) as u64;
    let key = rate_key(q);
    match limiter.check(&key, window_secs * 1000, limit).await {
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

/// 检查单条 budget 类 quota（daily/monthly_budget_usd）。
///
/// **不预扣** —— 仅 peek 当前用量与 limit 比较：
/// - current_micros < limit * 1_000_000 → 放行
/// - 否则 429
///
/// 用 micros（百万分之一 USD）做单位是为了在 Lua/Redis 里走整数路径
/// （和 D4 计费 outbox 的 cost_micros 对齐）。
async fn check_budget(qc: &Arc<gate_cache::QuotaCounter>, q: &QuotaRecord) -> Decision {
    let limit_micros = match (q.limit_value * Decimal::from(1_000_000)).to_i64() {
        Some(n) if n >= 0 => n,
        _ => {
            tracing::warn!(quota_id = %q.id, "budget quota limit invalid; fail-open");
            return Decision::Allowed;
        }
    };
    let key = budget_key(q);
    match qc.peek(&key).await {
        Ok(used) if used < limit_micros => Decision::Allowed,
        Ok(_) => Decision::Denied {
            dimension: q.dimension.clone(),
            // budget 不像 rate 有明确恢复时间——给一个保守的 1h 回退建议
            retry_after_ms: 60 * 60 * 1000,
        },
        Err(e) => {
            tracing::warn!(error = %e, quota_id = %q.id, "budget quota peek failed; fail-open");
            Decision::Allowed
        }
    }
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
            // 让真实 handler 报 401，由 extractor 路径处理
            let req = Request::from_parts(parts, body);
            return next.run(req).await;
        }
    };

    // anonymous 直接放行（未认证主体没有 quota 概念）
    if ctx.subject().is_none() {
        let req = Request::from_parts(parts, body);
        return next.run(req).await;
    }

    let scopes = scopes_for(&ctx);
    if scopes.is_empty() {
        let req = Request::from_parts(parts, body);
        return next.run(req).await;
    }

    // 收集所有 enabled 的 quota 行
    let mut quotas: Vec<QuotaRecord> = Vec::new();
    for s in &scopes {
        match state.repos.quotas.find_active_for(s.kind, s.id).await {
            Ok(mut rows) => quotas.append(&mut rows),
            Err(e) => {
                // DB 出错时 fail-open，避免把整站卡死
                tracing::warn!(error = %e, scope = %s.kind, id = %s.id, "quota load failed; fail-open");
            }
        }
    }
    if quotas.is_empty() {
        let req = Request::from_parts(parts, body);
        return next.run(req).await;
    }

    // 逐条评估
    for q in &quotas {
        let decision = match q.dimension.as_str() {
            "rpm" | "tpm" => match &state.rate_limiter {
                Some(rl) => check_rate(rl, q).await,
                None => Decision::Allowed,
            },
            "daily_budget_usd" | "monthly_budget_usd" => match &state.quota_counter {
                Some(qc) => check_budget(qc, q).await,
                None => Decision::Allowed,
            },
            // 其他维度（concurrent / lifetime_tokens）暂不在此层执行
            _ => Decision::Allowed,
        };
        if let Decision::Denied {
            dimension,
            retry_after_ms,
        } = decision
        {
            return quota_exceeded_response(&dimension, retry_after_ms).into_response();
        }
    }

    // 把已解析的 AuthContext 写进 extension，让下游 extractor 跳过重复解析
    // （Authed extractor 当前未读 extension，但留作未来优化口子）
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
    message: String,
    dimension: &'a str,
    retry_after_ms: u64,
}

fn quota_exceeded_response(dimension: &str, retry_after_ms: u64) -> impl IntoResponse + use<> {
    let secs = retry_after_ms.div_ceil(1000).max(1);
    let body = Json(ErrBody {
        error: ErrDetail {
            code: "quota_exceeded",
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
