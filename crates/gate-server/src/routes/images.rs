//! POST /v1/images/generations — 图片生成代理（OpenAI 兼容）。

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
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
    let provider = app
        .image_provider
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("image generation not configured".into()))?;

    let resp = provider
        .generate_image(req)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(resp))
}
