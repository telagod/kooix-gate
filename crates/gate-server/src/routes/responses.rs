//! POST /v1/responses — OpenAI Responses API thin adapter。
//!
//! 初版不复刻完整 Responses state machine，只把常用 Responses request
//! 映射为 `/v1/chat/completions` 同一条 provider / billing / quota 链路。

use crate::auth::Authed;
use crate::billing_emit::{BillingCtx, emit_usage};
use crate::error::{AppError, AppResult};
use crate::gateway::{GatewayStage, StageOutcome};
use crate::inflight::InflightGuards;
use crate::middleware::KooixRequestId;
use crate::routes::chat;
use crate::state::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{Extension, Json, Router, routing::post};
use futures::StreamExt;
use gate_core::id::ChannelId;
use gate_providers::retry::with_retry;
use gate_providers::{
    ChatMessage, ChatRequest, ChatResponse, ContentPart, ContentType, MessageContent, Role,
    ToolDef, Usage,
};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/responses", post(create_response))
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesRequest {
    model: String,
    input: ResponseInput,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default, alias = "max_completion_tokens")]
    max_output_tokens: Option<u32>,
    #[serde(default)]
    tools: Option<Vec<ToolDef>>,
    #[serde(default)]
    tool_choice: Option<serde_json::Value>,
    #[serde(default, flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ResponseInput {
    Text(String),
    Items(Vec<ResponseInputItem>),
}

#[derive(Debug, Clone, Deserialize)]
struct ResponseInputItem {
    #[serde(default)]
    role: Option<Role>,
    #[serde(default)]
    content: Option<ResponseInputContent>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ResponseInputContent {
    Text(String),
    Parts(Vec<ResponseContentPart>),
}

#[derive(Debug, Clone, Deserialize)]
struct ResponseContentPart {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    image_url: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
struct ResponsesResponse {
    id: String,
    object: &'static str,
    created_at: i64,
    status: &'static str,
    model: String,
    output: Vec<ResponseOutputItem>,
    output_text: String,
    #[serde(default)]
    usage: Usage,
}

#[derive(Debug, Clone, Serialize)]
struct ResponseOutputItem {
    #[serde(rename = "type")]
    kind: &'static str,
    id: String,
    status: &'static str,
    role: &'static str,
    content: Vec<ResponseOutputContent>,
}

#[derive(Debug, Clone, Serialize)]
struct ResponseOutputContent {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
}

async fn create_response(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
    guards: Option<Extension<InflightGuards>>,
    Json(req): Json<ResponsesRequest>,
) -> AppResult<axum::response::Response> {
    let mut chat_req = responses_to_chat_request(req)?;
    let route_start = std::time::Instant::now();
    let (
        provider,
        channel_id,
        retry_config,
        params_override,
        provider_type,
        routed_metrics,
        routed_key_id,
        routed_model,
    ) = chat::resolve_provider(&app, &ctx, &headers, &chat_req).await?;
    crate::gateway::record_stage(
        GatewayStage::Route,
        StageOutcome::Ok,
        route_start.elapsed().as_secs_f64(),
    );

    let adapt_start = std::time::Instant::now();
    chat::apply_params_override(&mut chat_req, &params_override);
    if let Some(model) = routed_model {
        chat_req.model = model;
    }
    gate_providers::adapt::adapt_for_provider(&mut chat_req, &provider_type);
    crate::gateway::record_stage(
        GatewayStage::Adapt,
        StageOutcome::Ok,
        adapt_start.elapsed().as_secs_f64(),
    );

    let request_id = request_id
        .map(|Extension(id)| id.0)
        .unwrap_or_else(Uuid::now_v7);
    let billing_ctx = BillingCtx::from_auth(&ctx, channel_id, &chat_req.model, request_id);

    if chat_req.stream {
        create_response_stream(
            app,
            provider,
            channel_id,
            routed_key_id,
            routed_metrics,
            billing_ctx,
            guards,
            chat_req,
        )
        .await
    } else {
        create_response_non_stream(
            app,
            provider,
            channel_id,
            retry_config,
            routed_key_id,
            routed_metrics,
            billing_ctx,
            guards,
            chat_req,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn create_response_non_stream(
    app: AppState,
    provider: std::sync::Arc<dyn gate_providers::Provider>,
    channel_id: Option<Uuid>,
    retry_config: gate_providers::retry::RetryConfig,
    routed_key_id: Option<gate_core::id::ChannelKeyId>,
    routed_metrics: Option<std::sync::Arc<gate_providers::ChannelMetrics>>,
    billing_ctx: Option<BillingCtx>,
    guards: Option<Extension<InflightGuards>>,
    chat_req: ChatRequest,
) -> AppResult<axum::response::Response> {
    let start = std::time::Instant::now();
    let model_for_metrics = chat_req.model.clone();
    let resp: ChatResponse = match with_retry(&retry_config, || {
        let req_clone = chat_req.clone();
        let provider = provider.clone();
        let app = app.clone();
        let routed_metrics = routed_metrics.clone();
        async move {
            match provider.chat(req_clone).await {
                Ok(resp) => Ok(resp),
                Err(err) => {
                    chat::report_channel_failure(
                        &app,
                        channel_id,
                        routed_key_id,
                        &err,
                        &routed_metrics,
                    )
                    .await;
                    Err(err)
                }
            }
        }
    })
    .await
    {
        Ok(resp) => {
            crate::gateway::record_stage(
                GatewayStage::Execute,
                StageOutcome::Ok,
                start.elapsed().as_secs_f64(),
            );
            if let (Some(m), Some(ch_uuid)) = (&routed_metrics, channel_id) {
                let ch_id = ChannelId::from(ch_uuid);
                m.record(ch_id, true);
                m.record_latency(ch_id, start.elapsed().as_millis() as u64);
            }
            chat::report_channel_success(&app, routed_key_id).await;
            resp
        }
        Err(e) => {
            crate::gateway::record_stage(
                GatewayStage::Execute,
                StageOutcome::Error,
                start.elapsed().as_secs_f64(),
            );
            release_channel(&app, channel_id);
            return Err(AppError::Provider(e));
        }
    };

    release_channel(&app, channel_id);
    if let (Some(router), Some(ch_uuid)) = (&app.provider_router, channel_id) {
        router
            .rate_limiter()
            .record_tokens(ChannelId::from(ch_uuid), chat::metered_tokens(&resp.usage))
            .await;
    }
    crate::metrics::record_tokens(
        &model_for_metrics,
        resp.usage.prompt_tokens as u64,
        resp.usage.completion_tokens as u64,
    );

    if let Some(Extension(ref g)) = guards {
        chat::settle_guards(g, &resp.usage).await;
    }

    if let Some(bctx) = billing_ctx {
        let outbox = app.outbox.clone();
        let pricing = app.pricing.clone();
        let usage = resp.usage.clone();
        tokio::spawn(async move {
            emit_usage(outbox, pricing, bctx, usage, 200).await;
        });
    }

    Ok(Json(chat_to_responses_response(resp)).into_response())
}

#[allow(clippy::too_many_arguments)]
async fn create_response_stream(
    app: AppState,
    provider: std::sync::Arc<dyn gate_providers::Provider>,
    channel_id: Option<Uuid>,
    routed_key_id: Option<gate_core::id::ChannelKeyId>,
    routed_metrics: Option<std::sync::Arc<gate_providers::ChannelMetrics>>,
    billing_ctx: Option<BillingCtx>,
    guards: Option<Extension<InflightGuards>>,
    chat_req: ChatRequest,
) -> AppResult<axum::response::Response> {
    let model = chat_req.model.clone();
    let estimated_stream_usage = chat::estimated_usage_from_request(&chat_req);
    let execute_start = std::time::Instant::now();
    let upstream = match provider.chat_stream(chat_req).await {
        Ok(upstream) => {
            crate::gateway::record_stage(
                GatewayStage::Execute,
                StageOutcome::Ok,
                execute_start.elapsed().as_secs_f64(),
            );
            upstream
        }
        Err(e) => {
            crate::gateway::record_stage(
                GatewayStage::Execute,
                StageOutcome::Error,
                execute_start.elapsed().as_secs_f64(),
            );
            chat::report_channel_failure(&app, channel_id, routed_key_id, &e, &routed_metrics)
                .await;
            release_channel(&app, channel_id);
            return Err(AppError::Provider(e));
        }
    };

    if let (Some(m), Some(ch_uuid)) = (&routed_metrics, channel_id) {
        m.record(ChannelId::from(ch_uuid), true);
    }
    chat::report_channel_success(&app, routed_key_id).await;

    let app_for_tail = app.clone();
    let billing_ctx_for_tail = billing_ctx.clone();
    let guards_for_tail = guards.clone();
    let captured_usage = std::sync::Arc::new(parking_lot::Mutex::new(None::<Usage>));
    let captured_usage_for_tail = captured_usage.clone();
    let channel_for_tail = channel_id.map(ChannelId::from);
    let rate_limiter_for_tail = app
        .provider_router
        .as_ref()
        .map(|router| router.rate_limiter());
    let model_for_tail = model.clone();

    let mapped = upstream.map(move |item| match item {
        Ok(chunk) => {
            if let Some(usage) = &chunk.usage {
                *captured_usage.lock() = Some(usage.clone());
            }
            let delta = chunk
                .choices
                .first()
                .and_then(|choice| choice.delta.content.clone())
                .unwrap_or_default();
            let payload = serde_json::json!({
                "type": "response.output_text.delta",
                "delta": delta
            });
            Ok::<_, Infallible>(Event::default().data(payload.to_string()))
        }
        Err(e) => Ok(Event::default().data(
            serde_json::json!({
                "type": "error",
                "error": e.to_string()
            })
            .to_string(),
        )),
    });

    let tail = futures::stream::once(async move {
        let usage = captured_usage_for_tail
            .lock()
            .take()
            .unwrap_or(estimated_stream_usage);
        release_channel(&app_for_tail, channel_id);
        if let (Some(rl), Some(ch_id)) = (&rate_limiter_for_tail, channel_for_tail) {
            rl.record_tokens(ch_id, chat::metered_tokens(&usage)).await;
            crate::metrics::record_tokens(
                &model_for_tail,
                usage.prompt_tokens as u64,
                usage.completion_tokens as u64,
            );
        }
        if let Some(Extension(ref g)) = guards_for_tail {
            chat::settle_guards(g, &usage).await;
        }
        if let Some(bctx) = billing_ctx_for_tail {
            let outbox = app_for_tail.outbox.clone();
            let pricing = app_for_tail.pricing.clone();
            tokio::spawn(async move {
                emit_usage(outbox, pricing, bctx, usage, 200).await;
            });
        }
        Ok::<_, Infallible>(
            Event::default().data(serde_json::json!({"type":"response.completed"}).to_string()),
        )
    });

    Ok(Sse::new(mapped.chain(tail))
        .keep_alive(KeepAlive::default())
        .into_response())
}

fn release_channel(app: &AppState, channel_id: Option<Uuid>) {
    if let (Some(router), Some(ch_uuid)) = (&app.provider_router, channel_id) {
        router.release_channel(ChannelId::from(ch_uuid));
    }
}

fn responses_to_chat_request(req: ResponsesRequest) -> AppResult<ChatRequest> {
    let mut messages = Vec::new();
    if let Some(instructions) = req.instructions
        && !instructions.trim().is_empty()
    {
        messages.push(ChatMessage::text(Role::System, instructions));
    }
    messages.extend(response_input_to_messages(req.input)?);
    if messages.is_empty() {
        return Err(AppError::BadRequest(
            "responses input must not be empty".into(),
        ));
    }

    Ok(ChatRequest {
        model: req.model,
        messages,
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens: req.max_output_tokens,
        stream: req.stream,
        tools: req.tools,
        tool_choice: req.tool_choice,
        extra: req.extra,
    })
}

fn response_input_to_messages(input: ResponseInput) -> AppResult<Vec<ChatMessage>> {
    match input {
        ResponseInput::Text(text) => Ok(vec![ChatMessage::text(Role::User, text)]),
        ResponseInput::Items(items) => items
            .into_iter()
            .map(|item| {
                let role = item.role.unwrap_or(Role::User);
                let content = match item.content {
                    Some(content) => response_content_to_message_content(content)?,
                    None => MessageContent::Text(String::new()),
                };
                Ok(ChatMessage {
                    role,
                    content: Some(content),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                })
            })
            .collect(),
    }
}

fn response_content_to_message_content(content: ResponseInputContent) -> AppResult<MessageContent> {
    match content {
        ResponseInputContent::Text(text) => Ok(MessageContent::Text(text)),
        ResponseInputContent::Parts(parts) => {
            let mut out = Vec::new();
            for part in parts {
                match part.kind.as_str() {
                    "input_text" | "text" => out.push(ContentPart::Text {
                        r#type: ContentType::Text,
                        text: part.text.unwrap_or_default(),
                    }),
                    "input_image" | "image_url" => {
                        let url =
                            part.image_url
                                .and_then(|value| {
                                    value.as_str().map(ToOwned::to_owned).or_else(|| {
                                        value.get("url")?.as_str().map(ToOwned::to_owned)
                                    })
                                })
                                .ok_or_else(|| {
                                    AppError::BadRequest(
                                        "responses image input requires image_url".into(),
                                    )
                                })?;
                        out.push(ContentPart::ImageUrl {
                            r#type: ContentType::ImageUrl,
                            image_url: gate_providers::ImageUrl { url, detail: None },
                        });
                    }
                    other => {
                        return Err(AppError::BadRequest(format!(
                            "unsupported responses input content type '{other}'"
                        )));
                    }
                }
            }
            Ok(MessageContent::Parts(out))
        }
    }
}

fn chat_to_responses_response(resp: ChatResponse) -> ResponsesResponse {
    let output_text = resp
        .choices
        .first()
        .and_then(|choice| choice.message.content.as_ref())
        .map(MessageContent::to_text)
        .unwrap_or_default();
    ResponsesResponse {
        id: resp.id.clone(),
        object: "response",
        created_at: chrono::Utc::now().timestamp(),
        status: "completed",
        model: resp.model,
        output: vec![ResponseOutputItem {
            kind: "message",
            id: format!("msg_{}", Uuid::now_v7().simple()),
            status: "completed",
            role: "assistant",
            content: vec![ResponseOutputContent {
                kind: "output_text",
                text: output_text.clone(),
            }],
        }],
        output_text,
        usage: resp.usage,
    }
}
