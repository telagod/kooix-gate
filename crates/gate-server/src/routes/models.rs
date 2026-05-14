//! GET /v1/models — 动态模型列表。
//!
//! 聚合所有 active channel 的 supported_models，去重返回。

use crate::auth::Authed;
use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::State;
use axum::{Json, Router, routing::get};
use gate_providers::types::{ModelInfo, ModelListResponse};
use std::collections::HashSet;

pub fn router() -> Router<AppState> {
    Router::new().route("/models", get(list_models))
}

async fn list_models(
    State(app): State<AppState>,
    Authed(_ctx): Authed,
) -> AppResult<Json<ModelListResponse>> {
    let channels = app.repos.channels.list_admin_view().await?;
    let mut seen = HashSet::new();
    let mut models = Vec::new();

    for ch in &channels {
        if ch.status != "active" {
            continue;
        }
        if ch.supported_models.is_empty() {
            continue;
        }
        for m in &ch.supported_models {
            if seen.insert(m.clone()) {
                models.push(ModelInfo {
                    id: m.clone(),
                    object: "model".to_string(),
                    created: ch.created_at.timestamp(),
                    owned_by: ch.provider_type.clone(),
                });
            }
        }
    }

    models.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(Json(ModelListResponse {
        object: "list".to_string(),
        data: models,
    }))
}
