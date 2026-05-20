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
use gate_providers::{ChatRequest, ChatResponse, Usage};
mod responses_codec;
use responses_codec::{ResponsesRequest, chat_to_responses_response, responses_to_chat_request};
use std::convert::Infallible;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/responses", post(create_response))
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
        routed_group_id,
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
    let billing_ctx = BillingCtx::from_auth(
        &ctx,
        channel_id,
        routed_group_id,
        &chat_req.model,
        request_id,
    );

    if chat_req.stream {
        create_response_stream(
            app,
            provider,
            channel_id,
            routed_key_id,
            routed_metrics,
            billing_ctx,
            guards,
            provider_type,
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
            provider_type,
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
    provider_type: String,
    chat_req: ChatRequest,
) -> AppResult<axum::response::Response> {
    let start = std::time::Instant::now();
    let model_for_metrics = chat_req.model.clone();
    let resp: ChatResponse = match with_retry(&retry_config, || {
        let req_clone = chat_req.clone();
        let provider = provider.clone();
        let app = app.clone();
        let routed_metrics = routed_metrics.clone();
        let provider_type = provider_type.clone();
        let model_for_metrics = model_for_metrics.clone();
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
                        &provider_type,
                        &model_for_metrics,
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
            chat::record_channel_success_observation(
                &app,
                channel_id,
                &routed_metrics,
                start.elapsed().as_millis() as u64,
                "request",
            )
            .await;
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
    provider_type: String,
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
            chat::report_channel_failure(
                &app,
                channel_id,
                routed_key_id,
                &e,
                &routed_metrics,
                &provider_type,
                &model,
            )
            .await;
            release_channel(&app, channel_id);
            return Err(AppError::Provider(e));
        }
    };

    chat::record_channel_success_observation(
        &app,
        channel_id,
        &routed_metrics,
        execute_start.elapsed().as_millis() as u64,
        "request",
    )
    .await;
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
    let provider_type_for_stream = provider_type.clone();
    let model_for_stream_errors = model.clone();

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
        Err(e) => {
            let channel = crate::metrics::channel_label(channel_id);
            let failure = crate::error::provider_failure_policy(&e);
            crate::metrics::record_upstream_error_with_context(
                failure.kind_label,
                &provider_type_for_stream,
                &channel,
                &model_for_stream_errors,
            );
            Ok(Event::default().data(
                serde_json::json!({
                    "type": "error",
                    "error": e.to_string()
                })
                .to_string(),
            ))
        }
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
