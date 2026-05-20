//! POST /v1/images/generations — 图片生成代理（OpenAI 兼容）。
//!
//! Provider 选路优先级：
//! 1. 若 AppState 有 provider_router，用它按 project_id 选 image-capable channel。
//!    - project_id 来源：API key 主体直接取；User 主体从 X-Kooix-Project 头取。
//! 2. 路由器找不到可用 channel → fallback 到 AppState.image_provider。
//! 3. 均无 → 400 Bad Request。

use crate::auth::Authed;
use crate::billing_emit::{BillingCtx, emit_usage};
use crate::error::{AppError, AppResult, provider_failure_policy};
use crate::gateway::{GatewayStage, StageOutcome};
use crate::inflight::{InflightGuards, QuotaMetric};
use crate::middleware::KooixRequestId;
use crate::state::AppState;
use crate::trace_context::{DataPlaneTrace, TraceIdentity, record_upstream_outcome};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::{Extension, Json, Router, routing::post};
use gate_auth::AuthError;
use gate_auth::context::Subject;
use gate_core::id::{ChannelId, ChannelKeyId, ProjectId};
use gate_providers::types::{ImageGenerationRequest, ImageGenerationResponse};
use gate_providers::{ImageProvider, ProviderError, RoutedImageProvider, Usage};
use std::sync::Arc;
use std::time::Instant;
use tracing::Instrument;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/images/generations", post(create_image))
}

async fn create_image(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
    guards: Option<Extension<InflightGuards>>,
    Json(mut req): Json<ImageGenerationRequest>,
) -> AppResult<Json<ImageGenerationResponse>> {
    let route_start = std::time::Instant::now();
    let (provider, channel_id, routed_group_id, routed_key_id, routed_model, provider_type) =
        resolve_image_provider(&app, &ctx, &headers, &req).await?;
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
    let trace = DataPlaneTrace::new(
        TraceIdentity::from_auth(&ctx, request_id),
        "images.generations",
        provider_type.clone(),
        channel_id,
        routed_group_id,
        req.model.clone(),
    );
    let data_span = trace.span();
    let billing_ctx =
        BillingCtx::from_auth(&ctx, channel_id, routed_group_id, &req.model, request_id);

    let execute_start = Instant::now();
    let upstream_span = data_span.in_scope(|| trace.upstream_span("generate_image", false));
    let upstream_start = Instant::now();
    let upstream_result = provider
        .generate_image(req.clone())
        .instrument(upstream_span.clone())
        .await;
    record_upstream_outcome(
        &upstream_span,
        if upstream_result.is_ok() {
            "ok"
        } else {
            "error"
        },
        upstream_start.elapsed(),
    );
    let resp = match upstream_result {
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
            report_image_key_failure(
                &app,
                routed_key_id,
                &e,
                &provider_type,
                channel_id,
                &req.model,
            )
            .await;
            return Err(AppError::Provider(e));
        }
    };

    let usage = image_usage(&req, &resp);
    if let Some(router) = &app.provider_router
        && let Some(ch_uuid) = channel_id
    {
        router.release_channel(ChannelId::from(ch_uuid));
    }
    report_image_key_success(&app, routed_key_id).await;

    if let Some(Extension(ref g)) = guards {
        settle_image_guards(g, &usage).await;
    }

    if let Some(bctx) = billing_ctx {
        let outbox = app.outbox.clone();
        let pricing = app.pricing.clone();
        tokio::spawn(
            async move {
                emit_usage(outbox, pricing, bctx, usage, 200).await;
            }
            .instrument(data_span.clone()),
        );
    }

    Ok(Json(resp))
}

fn image_usage(req: &ImageGenerationRequest, resp: &ImageGenerationResponse) -> Usage {
    let requested = req.n.unwrap_or(1).max(1);
    let actual = (resp.data.len() as u32).max(1);
    let image_units = actual.max(requested);
    Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        image_units: Some(image_units),
        raw: Some(serde_json::json!({
            "endpoint": "images.generations",
            "image_units": image_units,
            "requested_n": requested,
            "returned_images": resp.data.len(),
            "quality": req.quality,
            "size": req.size,
            "style": req.style
        })),
        ..Default::default()
    }
}

async fn settle_image_guards(guards: &InflightGuards, usage: &Usage) {
    const RATE_PER_IMAGE_MICROS: i64 = 80_000;
    let actual_cost = usage.image_units.unwrap_or(1).max(1) as i64 * RATE_PER_IMAGE_MICROS;
    let mut taken = guards.take();
    for g in &mut taken {
        let actual = match g.metric {
            QuotaMetric::CostMicros => actual_cost,
            QuotaMetric::Tokens | QuotaMetric::Concurrent => 0,
        };
        g.settle_units(actual).await;
    }
}

async fn resolve_image_provider(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    headers: &HeaderMap,
    req: &ImageGenerationRequest,
) -> AppResult<(
    Arc<dyn ImageProvider>,
    Option<Uuid>,
    Option<Uuid>,
    Option<ChannelKeyId>,
    Option<String>,
    String,
)> {
    if let Some(router) = &app.provider_router {
        let project_id_opt = extract_project_id(app, ctx, headers).await?;

        if let Some(project_id) = project_id_opt {
            match router.route_image(project_id, &req.model).await {
                Ok(Some(RoutedImageProvider {
                    provider,
                    channel_id,
                    group_id,
                    key_id,
                    resolved_model,
                    provider_type,
                    ..
                })) => {
                    return Ok((
                        provider,
                        Some(*channel_id.as_uuid()),
                        Some(*group_id.as_uuid()),
                        key_id,
                        Some(resolved_model),
                        provider_type,
                    ));
                }
                Ok(None) => {
                    tracing::debug!(
                        project_id = %project_id,
                        model = %req.model,
                        "provider_router returned None for image generation, trying fallback"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "provider_router error for image generation");
                    return Err(AppError::Provider(e));
                }
            }
        }
    }

    if let Some(provider) = &app.image_provider {
        return Ok((
            provider.clone(),
            None,
            None,
            None,
            None,
            "fallback".to_string(),
        ));
    }

    Err(AppError::NoRoute {
        capability: "image",
        model: req.model.clone(),
    })
}

async fn report_image_key_success(app: &AppState, key_id: Option<ChannelKeyId>) {
    let Some(key_id) = key_id else {
        return;
    };
    if let Err(e) = app.repos.channel_keys.report_success(key_id).await {
        tracing::warn!(channel_key_id = %key_id.as_uuid(), error = %e, "image channel key success report failed");
    }
}

async fn report_image_key_failure(
    app: &AppState,
    key_id: Option<ChannelKeyId>,
    error: &ProviderError,
    provider_type: &str,
    channel_id: Option<Uuid>,
    model: &str,
) {
    let failure = provider_failure_policy(error);
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
        tracing::warn!(channel_key_id = %key_id.as_uuid(), error = %e, "image channel key failure report failed");
    }
    let channel = crate::metrics::channel_label(channel_id);
    crate::metrics::record_upstream_error_with_context(
        failure.kind_label,
        provider_type,
        &channel,
        model,
    );
}

async fn extract_project_id(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    headers: &HeaderMap,
) -> AppResult<Option<ProjectId>> {
    if let Some(Subject::ApiKey { project_id, .. }) = ctx.subject() {
        return Ok(Some(*project_id));
    }

    let Some(raw) = headers.get("x-kooix-project").and_then(|v| v.to_str().ok()) else {
        return Ok(None);
    };

    let project_id: ProjectId = raw
        .trim()
        .parse()
        .map_err(|_| AppError::BadRequest("invalid X-Kooix-Project".into()))?;

    let project = app.repos.projects.find_by_id(project_id).await?;
    let Some(org) = ctx.current_org() else {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "image.use_project".into(),
            resource: format!("project:{}", project_id.as_uuid()),
        }));
    };
    if project.org_id != org {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "image.use_project".into(),
            resource: format!("project:{}", project_id.as_uuid()),
        }));
    }

    if !ctx.is_super_admin()
        && ctx.project_role(&org, &project_id).is_none()
        && ctx.org_role(&org).is_none()
    {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "image.use_project".into(),
            resource: format!("project:{}", project_id.as_uuid()),
        }));
    }

    Ok(Some(project_id))
}
