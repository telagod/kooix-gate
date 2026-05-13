//! /v1/chat/completions — LLM chat 入口（OpenAI 兼容）
//!
//! 现阶段：
//! - 单一 Provider（state.provider），后续路由表上线后按 model 选 provider
//! - 鉴权：API key 或 User token 都能用
//! - 限流走 /v1 layer 的 middleware
//! - 流式：SSE 透传，每个 chunk 序列化为 `data: {json}\n\n`，结束 `data: [DONE]\n\n`

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use futures::stream::StreamExt;
use gate_providers::{ChatRequest, ChatResponse};
use std::convert::Infallible;

pub fn router() -> Router<AppState> {
    Router::new().route("/chat/completions", post(chat_completions))
}

async fn chat_completions(
    State(app): State<AppState>,
    Authed(_ctx): Authed,
    Json(req): Json<ChatRequest>,
) -> AppResult<axum::response::Response> {
    let provider = app
        .provider
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("no provider configured".into()))?
        .clone();

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
