//! POST /v1/embeddings — Embedding 代理。
//!
//! Provider 选路优先级（与 chat.rs 一致）：
//! 1. 若 AppState 有 provider_router，用它按 project_id 选 channel（route_embedding）
//!    - project_id 来源：API key 主体直接取；User 主体从 X-Kooix-Project 头取
//! 2. 路由器找不到可用 channel → 400 Bad Request
//!
//! 说明：AppState.provider 字段是 `Arc<dyn Provider>`，当前无法安全下转为
//! `EmbeddingProvider`，所以 embedding 不走全局 provider fallback。

use crate::auth::Authed;
use crate::billing_emit::{BillingCtx, emit_usage};
use crate::error::{AppError, AppResult};
use crate::gateway::{GatewayStage, StageOutcome};
use crate::inflight::InflightGuards;
use crate::middleware::KooixRequestId;
use crate::state::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::{Extension, Json, Router, routing::post};
use gate_auth::AuthError;
use gate_auth::context::Subject;
use gate_core::id::{ChannelId, ChannelKeyId, ProjectId};
use gate_providers::types::{EmbeddingRequest, EmbeddingResponse};
use gate_providers::{EmbeddingProvider, ProviderError, RoutedEmbeddingProvider, Usage};
use std::sync::Arc;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/embeddings", post(create_embedding))
}

async fn create_embedding(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
    guards: Option<Extension<InflightGuards>>,
    Json(mut req): Json<EmbeddingRequest>,
) -> AppResult<Json<EmbeddingResponse>> {
    let route_start = std::time::Instant::now();
    let (provider, channel_id, routed_key_id, routed_model) =
        resolve_embedding_provider(&app, &ctx, &headers, &req).await?;
    crate::gateway::record_stage(
        GatewayStage::Route,
        StageOutcome::Ok,
        route_start.elapsed().as_secs_f64(),
    );
    if let Some(model) = routed_model {
        req.model = model;
    }
    let request_id = request_id
        .map(|Extension(id)| id.0)
        .unwrap_or_else(Uuid::now_v7);
    let billing_ctx = BillingCtx::from_auth(&ctx, channel_id, &req.model, request_id);
    let execute_start = std::time::Instant::now();
    let resp = match provider.embed(req.clone()).await {
        Ok(resp) => {
            crate::gateway::record_stage(
                GatewayStage::Execute,
                StageOutcome::Ok,
                execute_start.elapsed().as_secs_f64(),
            );
            resp
        }
        Err(e) => {
            crate::gateway::record_stage(
                GatewayStage::Execute,
                StageOutcome::Error,
                execute_start.elapsed().as_secs_f64(),
            );
            if let Some(router) = &app.provider_router
                && let Some(ch_uuid) = channel_id
            {
                router.release_channel(ChannelId::from(ch_uuid));
            }
            report_embedding_key_failure(&app, routed_key_id, &e).await;
            return Err(AppError::Provider(e));
        }
    };

    let usage = embedding_usage(&resp);
    if let (Some(router), Some(ch_uuid)) = (&app.provider_router, channel_id) {
        let ch_id = ChannelId::from(ch_uuid);
        router
            .rate_limiter()
            .record_tokens(ch_id, usage.total_tokens)
            .await;
    }
    if let Some(router) = &app.provider_router
        && let Some(ch_uuid) = channel_id
    {
        router.release_channel(ChannelId::from(ch_uuid));
    }
    report_embedding_key_success(&app, routed_key_id).await;
    crate::metrics::record_tokens(&req.model, usage.prompt_tokens as u64, 0);

    if let Some(Extension(ref g)) = guards {
        settle_embedding_guards(g, &usage).await;
    }

    if let Some(bctx) = billing_ctx {
        let outbox = app.outbox.clone();
        let pricing = app.pricing.clone();
        let usage = Usage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: 0,
            total_tokens: usage.total_tokens,
            raw: Some(serde_json::json!({
                "endpoint": "embeddings",
                "prompt_tokens": usage.prompt_tokens,
                "total_tokens": usage.total_tokens
            })),
            ..Default::default()
        };
        tokio::spawn(async move {
            emit_usage(outbox, pricing, bctx, usage, 200).await;
        });
    }
    Ok(Json(resp))
}

#[derive(Debug, Clone, Copy)]
struct EmbeddingMeter {
    prompt_tokens: u32,
    total_tokens: u32,
}

fn embedding_usage(resp: &EmbeddingResponse) -> EmbeddingMeter {
    EmbeddingMeter {
        prompt_tokens: resp.usage.prompt_tokens,
        total_tokens: resp.usage.total_tokens.max(resp.usage.prompt_tokens),
    }
}

async fn settle_embedding_guards(guards: &InflightGuards, usage: &EmbeddingMeter) {
    const RATE_PER_TOKEN_MICROS: i64 = crate::cost_estimate::DEFAULT_RATE_PER_TOKEN_MICROS;
    let actual = usage.total_tokens as i64 * RATE_PER_TOKEN_MICROS;
    let mut taken = guards.take();
    for g in &mut taken {
        g.settle(actual).await;
    }
}

/// 按 subject 类型解析 project_id，经 ProviderRouter 选 EmbeddingProvider。
///
/// 返回顺序：
/// 1. ProviderRouter 选到 → 返回 provider
/// 2. ProviderRouter 找不到（返回 None） → 400
/// 3. ProviderRouter 配置 / 解密 / 构造失败 → provider error shape
async fn resolve_embedding_provider(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    headers: &HeaderMap,
    req: &EmbeddingRequest,
) -> AppResult<(
    Arc<dyn EmbeddingProvider>,
    Option<Uuid>,
    Option<ChannelKeyId>,
    Option<String>,
)> {
    if let Some(router) = &app.provider_router {
        let project_id_opt = extract_project_id(app, ctx, headers).await?;

        if let Some(project_id) = project_id_opt {
            match router.route_embedding(project_id, &req.model).await {
                Ok(Some(RoutedEmbeddingProvider {
                    provider,
                    channel_id,
                    key_id,
                    resolved_model,
                    ..
                })) => {
                    return Ok((
                        provider,
                        Some(*channel_id.as_uuid()),
                        key_id,
                        Some(resolved_model),
                    ));
                }
                Ok(None) => {
                    tracing::debug!(
                        project_id = %project_id,
                        model = %req.model,
                        "provider_router returned None for embedding, trying fallback"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "provider_router error for embedding");
                    return Err(AppError::Provider(e));
                }
            }
        }
    }

    // gate-providers 的 OpenAiProvider 等同时实现 Provider + EmbeddingProvider，
    // 但 AppState.provider 字段类型是 Arc<dyn Provider>，无法直接转型。
    // 因此 embedding 暂不走全局 provider fallback；没有匹配 channel 就返回清晰错误。
    Err(AppError::BadRequest(format!(
        "no embedding channel found for model '{}'",
        req.model
    )))
}

async fn report_embedding_key_success(app: &AppState, key_id: Option<ChannelKeyId>) {
    let Some(key_id) = key_id else {
        return;
    };
    if let Err(e) = app.repos.channel_keys.report_success(key_id).await {
        tracing::warn!(channel_key_id = %key_id.as_uuid(), error = %e, "embedding channel key success report failed");
    }
}

async fn report_embedding_key_failure(
    app: &AppState,
    key_id: Option<ChannelKeyId>,
    error: &ProviderError,
) {
    let failure = embedding_failure_policy(error);
    if let Some(key_id) = key_id
        && let Err(e) = app
            .repos
            .channel_keys
            .report_failure(
                key_id,
                failure.error_code,
                failure.cooldown_secs,
                failure.circuit_breaker_failures,
            )
            .await
    {
        tracing::warn!(channel_key_id = %key_id.as_uuid(), error = %e, "embedding channel key failure report failed");
    }
    crate::metrics::record_upstream_error(failure.kind_label);
}

struct EmbeddingFailurePolicy {
    kind_label: &'static str,
    error_code: Option<i32>,
    cooldown_secs: i64,
    circuit_breaker_failures: u32,
}

fn embedding_failure_policy(error: &ProviderError) -> EmbeddingFailurePolicy {
    let (kind_label, error_code, cooldown_ms, circuit_breaker_failures) = match error {
        ProviderError::Auth(_) => ("authentication_error", Some(401), None, None),
        ProviderError::RateLimited { retry_after_ms } => {
            ("rate_limit_error", Some(429), *retry_after_ms, None)
        }
        ProviderError::InvalidRequest(_) => ("invalid_request_error", Some(400), None, None),
        ProviderError::Policy(_) => ("policy_error", Some(403), None, None),
        ProviderError::Upstream { status, .. } => (
            "upstream_error",
            Some((*status).into()),
            status.ge(&500).then_some(60_000),
            None,
        ),
        ProviderError::Mapped {
            status, metadata, ..
        } => {
            let label = match metadata.kind {
                gate_providers::error::NormalizedProviderErrorKind::Authentication => {
                    "authentication_error"
                }
                gate_providers::error::NormalizedProviderErrorKind::RateLimit => "rate_limit_error",
                gate_providers::error::NormalizedProviderErrorKind::InvalidRequest => {
                    "invalid_request_error"
                }
                gate_providers::error::NormalizedProviderErrorKind::Policy => "policy_error",
                gate_providers::error::NormalizedProviderErrorKind::Upstream => "upstream_error",
            };
            (
                label,
                status.map(i32::from),
                metadata.cooldown_ms.or(metadata.retry_after_ms),
                metadata.circuit_breaker_failures,
            )
        }
        ProviderError::Network(_) => ("network_error", None, Some(60_000), None),
        ProviderError::Decode(_) => ("decode_error", None, None, None),
        ProviderError::Config(_) => ("config_error", None, None, None),
    };

    EmbeddingFailurePolicy {
        kind_label,
        error_code,
        cooldown_secs: cooldown_ms
            .map(|ms| ms.div_ceil(1000).max(1) as i64)
            .unwrap_or(300),
        circuit_breaker_failures: circuit_breaker_failures.unwrap_or(3).max(1),
    }
}

/// 从 AuthContext + headers 提取 project_id（与 chat.rs 完全相同的越权校验逻辑）。
async fn extract_project_id(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    headers: &HeaderMap,
) -> AppResult<Option<ProjectId>> {
    if let Some(Subject::ApiKey { project_id, .. }) = ctx.subject() {
        return Ok(Some(*project_id));
    }

    // User 主体：从 X-Kooix-Project 头取 UUID
    let Some(raw) = headers.get("x-kooix-project").and_then(|v| v.to_str().ok()) else {
        return Ok(None);
    };

    let project_id: ProjectId = raw
        .trim()
        .parse()
        .map_err(|_| AppError::BadRequest("invalid X-Kooix-Project".into()))?;

    // 越权校验：project.org_id 必须匹配 ctx.current_org
    let project = app.repos.projects.find_by_id(project_id).await?;
    let Some(org) = ctx.current_org() else {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "embed.use_project".into(),
            resource: format!("project:{}", project_id.as_uuid()),
        }));
    };
    if project.org_id != org {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "embed.use_project".into(),
            resource: format!("project:{}", project_id.as_uuid()),
        }));
    }

    if !ctx.is_super_admin()
        && ctx.project_role(&org, &project_id).is_none()
        && ctx.org_role(&org).is_none()
    {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "embed.use_project".into(),
            resource: format!("project:{}", project_id.as_uuid()),
        }));
    }

    Ok(Some(project_id))
}
