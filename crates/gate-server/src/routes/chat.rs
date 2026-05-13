//! /v1/chat/completions — LLM chat 入口（OpenAI 兼容）
//!
//! Provider 选路优先级：
//! 1. 若 AppState 有 provider_router，用它按 project_id 选 channel
//!    - project_id 来源：
//!      a. API key 主体 → ctx.subject 里的 project_id
//!      b. User 主体 → 请求头 X-Kooix-Project（UUID 字符串）
//! 2. 路由器找不到可用 channel 时，fallback 到 AppState.provider
//! 3. 两者均无 → 400 Bad Request
//!
//! 限流：走 /v1 layer middleware，此处无需重复处理。
//! 流式：SSE 透传，每个 chunk 序列化为 `data: {json}\n\n`，结束 `data: [DONE]\n\n`。

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use futures::stream::StreamExt;
use gate_auth::context::Subject;
use gate_core::id::ProjectId;
use gate_providers::{ChatRequest, ChatResponse, Provider};
use std::convert::Infallible;
use std::sync::Arc;

pub fn router() -> Router<AppState> {
    Router::new().route("/chat/completions", post(chat_completions))
}

async fn chat_completions(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<ChatRequest>,
) -> AppResult<axum::response::Response> {
    let provider = resolve_provider(&app, &ctx, &req).await?;

    if req.stream {
        let upstream = provider.chat_stream(req).await?;
        let sse_stream = upstream.map(|item| {
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
        let resp: ChatResponse = provider.chat(req).await?;
        Ok(Json(resp).into_response())
    }
}

/// 按 subject 类型解析 project_id，再经 ProviderRouter 选 Provider。
///
/// 返回顺序：
/// 1. ProviderRouter 选到 → 返回
/// 2. ProviderRouter 找不到（返回 None） → fallback 到 AppState.provider
/// 3. 均无 → 400
async fn resolve_provider(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    req: &ChatRequest,
) -> AppResult<Arc<dyn Provider>> {
    // 尝试从 ProviderRouter 获取
    if let Some(router) = &app.provider_router {
        let project_id_opt = extract_project_id(ctx);

        if let Some(project_id) = project_id_opt {
            match router.route(project_id, &req.model).await {
                Ok(Some(p)) => return Ok(p),
                Ok(None) => {
                    // 路由找不到 channel，继续 fallback
                    tracing::debug!(
                        project_id = %project_id,
                        "provider_router returned None, trying fallback provider"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "provider_router error, falling back");
                    // 路由出错时 fallback，不直接 500（让 fallback provider 接管）
                }
            }
        }
    }

    // Fallback: 使用全局 provider
    app.provider
        .clone()
        .ok_or_else(|| AppError::BadRequest("no provider configured".into()))
}

/// 从 AuthContext 提取 project_id。
///
/// - API key 主体：直接从 subject 取（project_id 在 API key 绑定时已确定）
/// - User 主体：从请求头 X-Kooix-Project 取（UUID 格式）
///   若未提供则返回 None，使用全局 provider fallback
fn extract_project_id(ctx: &gate_auth::AuthContext) -> Option<ProjectId> {
    match ctx.subject() {
        Some(Subject::ApiKey { project_id, .. }) => Some(*project_id),
        // User 主体的 project_id 由调用方在 X-Kooix-Project header 里传，
        // 此处暂返回 None（需要 axum Parts 访问 header，改为 middleware 注入更干净）
        // C2 阶段可优化：在 Authed extractor 里加 Option<ProjectId> 字段
        _ => None,
    }
}
