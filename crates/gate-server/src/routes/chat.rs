//! /v1/chat/completions — LLM chat 入口（OpenAI 兼容）
//!
//! Provider 选路优先级：
//! 1. 若 AppState 有 provider_router，用它按 project_id 选 channel
//!    - project_id 来源：
//!      a. API key 主体 → ctx.subject 里的 project_id（API key 绑定时已确定）
//!      b. User 主体 → 请求头 `X-Kooix-Project`（UUID 字符串）
//!      必须校验：project.org_id == current_org && ctx 有 project_role
//!      （防止伪造 project_id 跨 Org 调他人 channel）
//! 2. 路由器找不到可用 channel 时，fallback 到 AppState.provider
//! 3. 两者均无 → 400 Bad Request
//!
//! 限流：走 /v1 layer middleware，此处无需重复处理。
//! 流式：SSE 透传，每个 chunk 序列化为 `data: {json}\n\n`，结束 `data: [DONE]\n\n`。
//!
//! 计费（D4）：
//! - 仅 ApiKey 主体计费（User 主体直调没有 api_key 归属）
//! - 非流式：response 拿到后 spawn 一个 task 推 usage_event 到 outbox
//! - 流式：包装 upstream stream，捕获最后一帧的 usage；stream 结束后 spawn 推送
//! - outbox / pricing 任一未挂 → warn-only，不阻断请求
//!
//! 配额结算（F3）：
//! - pre-debit guards 通过 request extension 传入
//! - 非流式：response 后立即 settle
//! - 流式：stream 结束后在 trigger 闭包里 settle

use crate::auth::Authed;
use crate::billing_emit::{BillingCtx, emit_usage};
use crate::cost_estimate::DEFAULT_RATE_PER_TOKEN_MICROS;
use crate::error::{AppError, AppResult};
use crate::inflight::InflightGuards;
use crate::state::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::post;
use axum::{Extension, Json, Router};
use futures::stream::StreamExt;
use gate_auth::AuthError;
use gate_auth::context::Subject;
use gate_core::id::ProjectId;
use gate_providers::{ChannelMetrics, ChatRequest, ChatResponse, Provider, Usage};
use gate_providers::retry::{RetryConfig, with_retry};
use gate_core::id::ChannelId;
use std::convert::Infallible;
use parking_lot::Mutex;
use std::sync::Arc;

pub fn router() -> Router<AppState> {
    Router::new().route("/chat/completions", post(chat_completions))
}

/// 按实际 usage 计算结算费用（micros）。
///
/// 使用与 pre-debit 相同的 rate，确保预扣和结算口径一致。
fn actual_cost_from_usage(usage: &Usage) -> i64 {
    let total = usage.prompt_tokens as i64 + usage.completion_tokens as i64;
    total * DEFAULT_RATE_PER_TOKEN_MICROS
}

/// 结算所有 inflight guards。
async fn settle_guards(guards: &InflightGuards, usage: &Usage) {
    let actual = actual_cost_from_usage(usage);
    let mut taken = guards.take();
    for g in &mut taken {
        g.settle(actual).await;
    }
}

async fn chat_completions(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    guards: Option<Extension<InflightGuards>>,
    Json(mut req): Json<ChatRequest>,
) -> AppResult<axum::response::Response> {
    let (provider, channel_id, retry_config, params_override, provider_type, routed_metrics) = resolve_provider(&app, &ctx, &headers, &req).await?;

    // Apply params_override from model alias (if any)
    apply_params_override(&mut req, &params_override);

    // Adapt params for the target provider (drop unsupported OpenAI params, inject required fields)
    gate_providers::adapt::adapt_for_provider(&mut req, &provider_type);

    // 计费上下文：仅 ApiKey 主体生成；channel_id 来自 ProviderRouter，fallback 路径为 None
    let billing_ctx = BillingCtx::from_auth(&ctx, channel_id, &req.model);
    let model = req.model.clone();

    if req.stream {
        let upstream = provider.chat_stream(req).await?;

        // 流式：上报成功（stream 开启意味着请求已被接受）
        if let (Some(m), Some(ch_uuid)) = (&routed_metrics, channel_id) {
            let ch_id = ChannelId::from(ch_uuid);
            m.record(ch_id, true);
            if m.should_disable(ch_id) {
                let repos = app.repos.clone();
                tokio::spawn(async move {
                    if let Err(e) = repos.channels.auto_disable(ch_id, "success rate below threshold").await {
                        tracing::warn!(channel_id = %ch_id.as_uuid(), error = %e, "auto_disable failed");
                    } else {
                        tracing::warn!(channel_id = %ch_id.as_uuid(), "auto-disabled channel due to low success rate");
                    }
                });
            }
        }

        // 累积流式 usage：包装 stream，inspect 每个 chunk，记下最后含 usage 的那个
        let captured_usage: Arc<Mutex<Option<Usage>>> = Arc::new(Mutex::new(None));
        let captured_clone = captured_usage.clone();

        let app_for_billing = app.clone();
        let billing_ctx_clone = billing_ctx.clone();

        // least_conn release: stream 结束后释放 inflight 计数
        let router_for_release = app.provider_router.clone();
        let release_channel_id = channel_id.map(ChannelId::from);

        // TPM record: stream 结束后记录 token 消耗
        let rate_limiter_for_tpm = app.provider_router.as_ref().map(|r| r.rate_limiter());
        let tpm_channel_id = channel_id.map(ChannelId::from);

        // 用 inspect 抓 chunk.usage；stream 关闭后由 wrapper drop 触发 emit
        let wrapped = upstream.inspect(move |item| {
            if let Ok(chunk) = item
                && let Some(u) = chunk.usage
            {
                *captured_clone.lock() = Some(u);
            }
        });

        // 用 StreamExt::chain 在 upstream 流尾巴接一段 trigger emit 的「副作用」流。
        // 副作用流自身不吐 chunk，只在被 poll 时 spawn emit 任务后返回 None。
        let trigger = futures::stream::once(async move {
            let usage = captured_usage.lock().take();

            // 释放 inflight 计数（least_conn 策略）
            if let (Some(router), Some(ch_id)) = (&router_for_release, release_channel_id) {
                router.release_channel(ch_id);
            }

            // 记录 token 消耗到 per-channel TPM 计数器
            if let (Some(rl), Some(ch_id)) = (&rate_limiter_for_tpm, tpm_channel_id) {
                if let Some(ref u) = usage {
                    let total_tokens = u.prompt_tokens + u.completion_tokens;
                    rl.record_tokens(ch_id, total_tokens).await;
                }
            }

            // 结算 inflight guards（F3）
            if let Some(ref u) = usage
                && let Some(Extension(ref g)) = guards
            {
                settle_guards(g, u).await;
            }
            // 没有 usage 帧 + 有 guard → Drop 会自动全额退还（guards 移入此闭包，
            // 闭包结束时 Drop）

            if let (Some(usage), Some(bctx)) = (usage, billing_ctx_clone) {
                let outbox = app_for_billing.outbox.clone();
                let pricing = app_for_billing.pricing.clone();
                tokio::spawn(async move {
                    emit_usage(outbox, pricing, bctx, usage, 200).await;
                });
            } else {
                tracing::debug!(
                    model = %model,
                    "stream finished without usage frame; skipping billing"
                );
            }
            // 占位返回值，会被 filter_map 过滤掉
            None::<gate_providers::ProviderResult<gate_providers::ChatStreamChunk>>
        })
        .filter_map(|x| async move { x });

        let combined = wrapped.chain(trigger);

        let sse_stream = combined.map(|item| {
            let payload = match item {
                Ok(chunk) => serde_json::to_string(&chunk)
                    .unwrap_or_else(|_| "{\"error\":\"encode\"}".into()),
                Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
            };
            Ok::<_, Infallible>(Event::default().data(payload))
        });

        Ok(Sse::new(sse_stream)
            .keep_alive(KeepAlive::default())
            .into_response())
    } else {
        let start = std::time::Instant::now();
        let resp: ChatResponse = match with_retry(&retry_config, || {
            let req_clone = req.clone();
            let provider = provider.clone();
            async move { provider.chat(req_clone).await }
        })
        .await
        {
            Ok(r) => {
                let elapsed_ms = start.elapsed().as_millis() as u64;
                // 上报成功 + 延迟
                if let (Some(m), Some(ch_uuid)) = (&routed_metrics, channel_id) {
                    let ch_id = ChannelId::from(ch_uuid);
                    m.record(ch_id, true);
                    m.record_latency(ch_id, elapsed_ms);
                    if m.should_disable(ch_id) {
                        let repos = app.repos.clone();
                        tokio::spawn(async move {
                            if let Err(e) = repos.channels.auto_disable(ch_id, "success rate below threshold").await {
                                tracing::warn!(channel_id = %ch_id.as_uuid(), error = %e, "auto_disable failed");
                            } else {
                                tracing::warn!(channel_id = %ch_id.as_uuid(), "auto-disabled channel due to low success rate");
                            }
                        });
                    }
                }
                r
            }
            Err(e) => {
                // 上报失败
                if let (Some(m), Some(ch_uuid)) = (&routed_metrics, channel_id) {
                    let ch_id = ChannelId::from(ch_uuid);
                    m.record(ch_id, false);
                    if m.should_disable(ch_id) {
                        let repos = app.repos.clone();
                        tokio::spawn(async move {
                            if let Err(de) = repos.channels.auto_disable(ch_id, "success rate below threshold").await {
                                tracing::warn!(channel_id = %ch_id.as_uuid(), error = %de, "auto_disable failed");
                            } else {
                                tracing::warn!(channel_id = %ch_id.as_uuid(), "auto-disabled channel due to low success rate");
                            }
                        });
                    }
                }
                return Err(AppError::Provider(e));
            }
        };

        // 释放 inflight 计数（least_conn 策略）
        if let (Some(router), Some(ch_uuid)) = (&app.provider_router, channel_id) {
            router.release_channel(ChannelId::from(ch_uuid));
        }

        // 记录 token 消耗到 per-channel TPM 计数器
        if let (Some(router), Some(ch_uuid)) = (&app.provider_router, channel_id) {
            let total_tokens = resp.usage.prompt_tokens + resp.usage.completion_tokens;
            let rl = router.rate_limiter();
            let ch_id = ChannelId::from(ch_uuid);
            rl.record_tokens(ch_id, total_tokens).await;
        }

        // 结算 inflight guards（F3）
        if let Some(Extension(ref g)) = guards {
            settle_guards(g, &resp.usage).await;
        }

        // 非流式：response 立刻拿到 usage，spawn 一个 task 推 outbox（不阻塞返回）
        if let Some(bctx) = billing_ctx {
            let usage = resp.usage;
            let outbox = app.outbox.clone();
            let pricing = app.pricing.clone();
            tokio::spawn(async move {
                emit_usage(outbox, pricing, bctx, usage, 200).await;
            });
        }
        Ok(Json(resp).into_response())
    }
}

/// 按 subject 类型解析 project_id，再经 ProviderRouter 选 Provider。
///
/// 返回顺序：
/// 1. ProviderRouter 选到 → 返回 `(Provider, Some(channel_id))`
/// 2. ProviderRouter 找不到（返回 None） → fallback 到 AppState.provider，channel_id=None
/// 3. 均无 → 400
async fn resolve_provider(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    headers: &HeaderMap,
    req: &ChatRequest,
) -> AppResult<(Arc<dyn Provider>, Option<uuid::Uuid>, RetryConfig, serde_json::Value, String, Option<Arc<ChannelMetrics>>)> {
    // 尝试从 ProviderRouter 获取
    if let Some(router) = &app.provider_router {
        let project_id_opt = extract_project_id(app, ctx, headers).await?;

        if let Some(project_id) = project_id_opt {
            match router.route(project_id, &req.model).await {
                Ok(Some(routed)) => {
                    let provider_type = routed.provider_type.clone();
                    let metrics = routed.metrics.clone();
                    return Ok((routed.provider, Some(*routed.channel_id.as_uuid()), routed.retry_config, routed.params_override, provider_type, metrics));
                }
                Ok(None) => {
                    tracing::debug!(
                        project_id = %project_id,
                        "provider_router returned None, trying fallback provider"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "provider_router error, falling back");
                }
            }
        }
    }

    // Fallback: 使用全局 provider，无 channel_id 归属
    let provider = app
        .provider
        .clone()
        .ok_or_else(|| AppError::BadRequest("no provider configured".into()))?;
    Ok((provider, None, RetryConfig::default(), serde_json::json!({}), "openai".to_string(), None))
}

/// 从 AuthContext + headers 提取 project_id（带越权校验）。
///
/// - API key 主体：直接从 subject 取（API key 绑定时已确定，无需再校验）
/// - User 主体：从请求头 `X-Kooix-Project` 取（UUID 格式）
///   * 缺失 → 返回 None（走 fallback provider）
///   * 格式错 → 400 BadRequest
///   * project 不存在 → 转 ctx.require Forbidden（避免泄露存在性）
///   * project.org_id 与 ctx.current_org 不一致 → 403
///   * ctx 在该 project 无任何角色 → 403
async fn extract_project_id(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    headers: &HeaderMap,
) -> AppResult<Option<ProjectId>> {
    if let Some(Subject::ApiKey { project_id, .. }) = ctx.subject() {
        return Ok(Some(*project_id));
    }

    // 只有 User 主体走到这里
    let Some(raw) = headers.get("x-kooix-project").and_then(|v| v.to_str().ok()) else {
        return Ok(None);
    };

    let project_uuid = uuid::Uuid::parse_str(raw.trim())
        .map_err(|_| AppError::BadRequest("invalid X-Kooix-Project: not a UUID".into()))?;
    let project_id = ProjectId::from(project_uuid);

    // 越权校验：project.org_id 必须匹配 ctx.current_org
    let project = app.repos.projects.find_by_id(project_id).await?;
    let Some(org) = ctx.current_org() else {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "chat.use_project".into(),
            resource: format!("project:{project_id}"),
        }));
    };
    if project.org_id != org {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "chat.use_project".into(),
            resource: format!("project:{project_id}"),
        }));
    }

    // 必须在该 project 有角色（SuperAdmin 短路）
    if !ctx.is_super_admin()
        && ctx.project_role(&org, &project_id).is_none()
        && ctx.org_role(&org).is_none()
    {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "chat.use_project".into(),
            resource: format!("project:{project_id}"),
        }));
    }

    Ok(Some(project_id))
}

/// Merge model alias params_override into ChatRequest.
/// Known fields: temperature, max_tokens, top_p. Others go into `extra` (flatmap).
fn apply_params_override(req: &mut ChatRequest, overrides: &serde_json::Value) {
    let obj = match overrides.as_object() {
        Some(o) if !o.is_empty() => o,
        _ => return,
    };
    if let Some(v) = obj.get("temperature").and_then(|v| v.as_f64()) {
        req.temperature = Some(v as f32);
    }
    if let Some(v) = obj.get("max_tokens").and_then(|v| v.as_u64()) {
        req.max_tokens = Some(v as u32);
    }
    if let Some(v) = obj.get("top_p").and_then(|v| v.as_f64()) {
        req.top_p = Some(v as f32);
    }
    // Other override keys go into the flattened extra map
    for (k, v) in obj {
        match k.as_str() {
            "temperature" | "max_tokens" | "top_p" => {} // already handled
            _ => { req.extra.insert(k.clone(), v.clone()); }
        }
    }
}
