//! POST /v1/images/generations — 图片生成代理（OpenAI 兼容）。

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::gateway::{GatewayStage, StageOutcome};
use crate::state::AppState;
use axum::extract::State;
use axum::{Json, Router, routing::post};
use gate_providers::types::{ImageGenerationRequest, ImageGenerationResponse};

pub fn router() -> Router<AppState> {
    Router::new().route("/images/generations", post(create_image))
}

async fn create_image(
    State(app): State<AppState>,
    Authed(_ctx): Authed,
    Json(req): Json<ImageGenerationRequest>,
) -> AppResult<Json<ImageGenerationResponse>> {
    let route_start = std::time::Instant::now();
    let provider = app
        .image_provider
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("image generation not configured".into()))?;
    crate::gateway::record_stage(
        GatewayStage::Route,
        StageOutcome::Ok,
        route_start.elapsed().as_secs_f64(),
    );

    let execute_start = std::time::Instant::now();
    let resp = match provider.generate_image(req).await {
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
            return Err(AppError::Internal(e.to_string()));
        }
    };

    Ok(Json(resp))
}
