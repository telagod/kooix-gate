//! 基于 [`RateLimiter`] 的 Axum middleware。
//!
//! 设计要点：
//! - Subject 识别优先级：ApiKeyId > UserId > client_ip（未知主体）
//! - 超限返回 429，body `{error:{code:"rate_limited", retry_after_ms}}`
//! - 限流不 panic：RateLimiter 失败 (Redis 宕) 时 fail-open，只 warn
//!   - 宁可放行也不阻断全站；后续可在 Config 里加开关切 fail-closed

use crate::state::AppState;
use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use gate_auth::{AuthContext, Subject};

/// 按 subject 分桶限流。
///
/// 用法：
/// ```ignore
/// .layer(from_fn_with_state(state.clone(), rate_limit_by_subject))
/// ```
pub async fn rate_limit_by_subject(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let cfg = state.rate_limit_cfg;
    let limiter = match &state.rate_limiter {
        Some(r) => r.clone(),
        None => return next.run(req).await, // 未配置 Redis，放行
    };

    let bucket = subject_bucket(req.extensions().get::<AuthContext>(), &req);
    match limiter.check(&bucket, cfg.window_ms, cfg.capacity).await {
        Ok(d) if d.allowed => {
            let mut resp = next.run(req).await;
            let headers = resp.headers_mut();
            headers.insert(
                "x-ratelimit-remaining",
                HeaderValue::from_str(&d.remaining.to_string())
                    .unwrap_or(HeaderValue::from_static("0")),
            );
            resp
        }
        Ok(d) => rate_limited_response(d.retry_after_ms).into_response(),
        Err(e) => {
            // fail-open: 拒绝服务比把整个站卡死更糟
            tracing::warn!(error = %e, bucket = %bucket, "rate limiter failed; allowing");
            next.run(req).await
        }
    }
}

fn subject_bucket(ctx: Option<&AuthContext>, req: &Request) -> String {
    // 1. API key → 最强身份
    if let Some(c) = ctx {
        match c.subject() {
            Some(Subject::ApiKey { api_key_id, .. }) => {
                return format!("rl:apikey:{}", api_key_id.as_uuid());
            }
            Some(Subject::User { user_id, .. }) => {
                return format!("rl:user:{}", user_id.as_uuid());
            }
            _ => {}
        }
    }

    // 2. 客户端 IP
    let ip = extract_ip(req).unwrap_or_else(|| "unknown".to_string());
    format!("rl:ip:{ip}")
}

fn extract_ip(req: &Request) -> Option<String> {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            req.headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        })
}

#[derive(serde::Serialize)]
struct ErrBody<'a> {
    error: ErrDetail<'a>,
}
#[derive(serde::Serialize)]
struct ErrDetail<'a> {
    code: &'a str,
    message: &'a str,
    retry_after_ms: u64,
}

fn rate_limited_response(retry_after_ms: u64) -> impl IntoResponse {
    let secs = retry_after_ms.div_ceil(1000).max(1);
    let body = Json(ErrBody {
        error: ErrDetail {
            code: "rate_limited",
            message: "too many requests",
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
