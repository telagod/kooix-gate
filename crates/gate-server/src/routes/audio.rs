//! POST /v1/audio/speech — TTS 代理（OpenAI 兼容）。
//! POST /v1/audio/transcriptions — STT 代理（OpenAI 兼容）。
//!
//! Provider 选路优先级：
//! 1. 若 AppState 有 provider_router，用它按 project_id 选 audio-capable channel。
//!    - project_id 来源：API key 主体直接取；User 主体从 X-Kooix-Project 头取。
//! 2. 路由器找不到可用 channel → fallback 到 AppState.audio_provider。
//! 3. 均无 → 400 Bad Request。

use crate::auth::Authed;
use crate::billing_emit::{BillingCtx, emit_usage};
use crate::error::{AppError, AppResult};
use crate::gateway::{GatewayStage, StageOutcome};
use crate::inflight::InflightGuards;
use crate::middleware::KooixRequestId;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Multipart, State};
use axum::http::{HeaderMap, header};
use axum::response::Response;
use axum::{Extension, Json, Router, routing::post};
use gate_auth::AuthError;
use gate_auth::context::Subject;
use gate_core::id::{ChannelId, ChannelKeyId, ProjectId};
use gate_providers::types::{AudioSpeechRequest, AudioTranscriptionResponse};
use gate_providers::{AudioProvider, ProviderError, RoutedAudioProvider, Usage};
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_TTS_RATE_PER_CHAR_MICROS: i64 = 1;
const DEFAULT_STT_REQUEST_MICROS: i64 = 10_000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/audio/speech", post(create_speech))
        .route("/audio/transcriptions", post(create_transcription))
}

async fn create_speech(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
    guards: Option<Extension<InflightGuards>>,
    Json(mut req): Json<AudioSpeechRequest>,
) -> AppResult<Response> {
    let route_start = std::time::Instant::now();
    let (provider, channel_id, routed_key_id, routed_model) =
        resolve_audio_provider(&app, &ctx, &headers, &req.model).await?;
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
    let content_type = audio_content_type(req.response_format.as_deref());

    let execute_start = std::time::Instant::now();
    let audio_bytes = match provider.speech(req.clone()).await {
        Ok(bytes) => {
            crate::gateway::record_stage(
                GatewayStage::Execute,
                StageOutcome::Ok,
                execute_start.elapsed().as_secs_f64(),
            );
            bytes
        }
        Err(e) => {
            crate::gateway::record_stage(
                GatewayStage::Execute,
                StageOutcome::Error,
                execute_start.elapsed().as_secs_f64(),
            );
            release_audio_channel(&app, channel_id);
            report_audio_key_failure(&app, routed_key_id, &e).await;
            return Err(AppError::Provider(e));
        }
    };

    let usage = speech_usage(&req, audio_bytes.len());
    release_audio_channel(&app, channel_id);
    report_audio_key_success(&app, routed_key_id).await;

    if let Some(Extension(ref g)) = guards {
        settle_speech_guards(g, &usage).await;
    }

    if let Some(bctx) = billing_ctx {
        let outbox = app.outbox.clone();
        let pricing = app.pricing.clone();
        tokio::spawn(async move {
            emit_usage(outbox, pricing, bctx, usage, 200).await;
        });
    }

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(audio_bytes))
        .unwrap())
}

async fn create_transcription(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
    guards: Option<Extension<InflightGuards>>,
    mut multipart: Multipart,
) -> AppResult<Json<AudioTranscriptionResponse>> {
    let mut audio_data: Option<bytes::Bytes> = None;
    let mut filename = "audio.wav".to_string();
    let mut model = "whisper-1".to_string();
    let mut language: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                if let Some(fname) = field.file_name() {
                    filename = fname.to_string();
                }
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                audio_data = Some(data);
            }
            "model" => {
                model = field
                    .text()
                    .await
                    .unwrap_or_else(|_| "whisper-1".to_string());
            }
            "language" => {
                let text = field.text().await.unwrap_or_default();
                if !text.is_empty() {
                    language = Some(text);
                }
            }
            _ => {}
        }
    }

    let audio = audio_data.ok_or_else(|| AppError::BadRequest("missing 'file' field".into()))?;
    let audio_len = audio.len();

    let route_start = std::time::Instant::now();
    let (provider, channel_id, routed_key_id, routed_model) =
        resolve_audio_provider(&app, &ctx, &headers, &model).await?;
    crate::gateway::record_stage(
        GatewayStage::Route,
        StageOutcome::Ok,
        route_start.elapsed().as_secs_f64(),
    );
    if let Some(routed) = routed_model {
        model = routed;
    }

    let request_id = request_id
        .map(|Extension(id)| id.0)
        .unwrap_or_else(Uuid::now_v7);
    let billing_ctx = BillingCtx::from_auth(&ctx, channel_id, &model, request_id);

    let execute_start = std::time::Instant::now();
    let resp = match provider
        .transcription(audio, filename.clone(), model.clone(), language.clone())
        .await
    {
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
            release_audio_channel(&app, channel_id);
            report_audio_key_failure(&app, routed_key_id, &e).await;
            return Err(AppError::Provider(e));
        }
    };

    let usage = transcription_usage(&model, &filename, language.as_deref(), audio_len);
    release_audio_channel(&app, channel_id);
    report_audio_key_success(&app, routed_key_id).await;

    if let Some(Extension(ref g)) = guards {
        settle_transcription_guards(g).await;
    }

    if let Some(bctx) = billing_ctx {
        let outbox = app.outbox.clone();
        let pricing = app.pricing.clone();
        tokio::spawn(async move {
            emit_usage(outbox, pricing, bctx, usage, 200).await;
        });
    }

    Ok(Json(resp))
}

fn audio_content_type(format: Option<&str>) -> &'static str {
    match format.unwrap_or("mp3") {
        "opus" => "audio/opus",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "pcm" => "audio/pcm",
        _ => "audio/mpeg",
    }
}

fn speech_usage(req: &AudioSpeechRequest, response_bytes: usize) -> Usage {
    let tts_characters = req.input.chars().count() as u32;
    Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        raw: Some(serde_json::json!({
            "endpoint": "audio.speech",
            "tts_characters": tts_characters,
            "response_bytes": response_bytes,
            "voice": req.voice,
            "response_format": req.response_format,
            "speed": req.speed
        })),
        ..Default::default()
    }
}

fn transcription_usage(
    model: &str,
    filename: &str,
    language: Option<&str>,
    audio_bytes: usize,
) -> Usage {
    Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        raw: Some(serde_json::json!({
            "endpoint": "audio.transcriptions",
            "model": model,
            "filename": filename,
            "language": language,
            "audio_bytes": audio_bytes,
            "metering": "per_request"
        })),
        ..Default::default()
    }
}

async fn settle_speech_guards(guards: &InflightGuards, usage: &Usage) {
    let chars = usage
        .raw
        .as_ref()
        .and_then(|v| v.get("tts_characters"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as i64;
    let actual = (chars * DEFAULT_TTS_RATE_PER_CHAR_MICROS)
        .clamp(0, crate::cost_estimate::MAX_ESTIMATE_MICROS);
    let mut taken = guards.take();
    for g in &mut taken {
        g.settle(actual).await;
    }
}

async fn settle_transcription_guards(guards: &InflightGuards) {
    let mut taken = guards.take();
    for g in &mut taken {
        g.settle(DEFAULT_STT_REQUEST_MICROS).await;
    }
}

async fn resolve_audio_provider(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    headers: &HeaderMap,
    model: &str,
) -> AppResult<(
    Arc<dyn AudioProvider>,
    Option<Uuid>,
    Option<ChannelKeyId>,
    Option<String>,
)> {
    if let Some(router) = &app.provider_router {
        let project_id_opt = extract_project_id(app, ctx, headers).await?;

        if let Some(project_id) = project_id_opt {
            match router.route_audio(project_id, model).await {
                Ok(Some(RoutedAudioProvider {
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
                        model = %model,
                        "provider_router returned None for audio, trying fallback"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "provider_router error for audio");
                    return Err(AppError::Provider(e));
                }
            }
        }
    }

    if let Some(provider) = &app.audio_provider {
        return Ok((provider.clone(), None, None, None));
    }

    Err(AppError::BadRequest(format!(
        "no audio channel found for model '{model}'"
    )))
}

fn release_audio_channel(app: &AppState, channel_id: Option<Uuid>) {
    if let Some(router) = &app.provider_router
        && let Some(ch_uuid) = channel_id
    {
        router.release_channel(ChannelId::from(ch_uuid));
    }
}

async fn report_audio_key_success(app: &AppState, key_id: Option<ChannelKeyId>) {
    let Some(key_id) = key_id else {
        return;
    };
    if let Err(e) = app.repos.channel_keys.report_success(key_id).await {
        tracing::warn!(channel_key_id = %key_id.as_uuid(), error = %e, "audio channel key success report failed");
    }
}

async fn report_audio_key_failure(
    app: &AppState,
    key_id: Option<ChannelKeyId>,
    error: &ProviderError,
) {
    let failure = audio_failure_policy(error);
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
        tracing::warn!(channel_key_id = %key_id.as_uuid(), error = %e, "audio channel key failure report failed");
    }
    crate::metrics::record_upstream_error(failure.kind_label);
}

struct AudioFailurePolicy {
    kind_label: &'static str,
    error_code: Option<i32>,
    cooldown_secs: i64,
    circuit_breaker_failures: u32,
}

fn audio_failure_policy(error: &ProviderError) -> AudioFailurePolicy {
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

    AudioFailurePolicy {
        kind_label,
        error_code,
        cooldown_secs: cooldown_ms
            .map(|ms| ms.div_ceil(1000).max(1) as i64)
            .unwrap_or(300),
        circuit_breaker_failures: circuit_breaker_failures.unwrap_or(3).max(1),
    }
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
            action: "audio.use_project".into(),
            resource: format!("project:{}", project_id.as_uuid()),
        }));
    };
    if project.org_id != org {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "audio.use_project".into(),
            resource: format!("project:{}", project_id.as_uuid()),
        }));
    }

    if !ctx.is_super_admin()
        && ctx.project_role(&org, &project_id).is_none()
        && ctx.org_role(&org).is_none()
    {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "audio.use_project".into(),
            resource: format!("project:{}", project_id.as_uuid()),
        }));
    }

    Ok(Some(project_id))
}
