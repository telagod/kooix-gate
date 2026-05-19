//! GET /v1/models — 动态模型列表。
//!
//! 聚合所有 active + healthy channel 的 supported_models 与真实 runtime capability，去重返回。

use crate::auth::Authed;
use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::State;
use axum::{Json, Router, routing::get};
use gate_providers::types::{ModelInfo, ModelListResponse};
use gate_providers::{ProviderCapabilities, plugin_manifest, provider_capabilities};
use gate_storage::ChannelRecord;
use std::collections::BTreeMap;

pub fn router() -> Router<AppState> {
    Router::new().route("/models", get(list_models))
}

async fn list_models(
    State(app): State<AppState>,
    Authed(_ctx): Authed,
) -> AppResult<Json<ModelListResponse>> {
    let channels = app.repos.channels.list_admin_view().await?;
    let mut models: BTreeMap<String, ModelInfo> = BTreeMap::new();

    for ch in &channels {
        if !ch.is_healthy() || ch.supported_models.is_empty() {
            continue;
        }
        let caps = channel_capabilities(ch);
        for m in &ch.supported_models {
            models
                .entry(m.clone())
                .and_modify(|info| {
                    if let Some(existing) = &mut info.capabilities {
                        existing.merge_truthy(&caps);
                    }
                    if ch.created_at.timestamp() < info.created {
                        info.created = ch.created_at.timestamp();
                    }
                })
                .or_insert_with(|| ModelInfo {
                    id: m.clone(),
                    object: "model".to_string(),
                    created: ch.created_at.timestamp(),
                    owned_by: ch.provider_type.clone(),
                    capabilities: Some(caps.clone()),
                });
        }
    }

    Ok(Json(ModelListResponse {
        object: "list".to_string(),
        data: models.into_values().collect(),
    }))
}

fn channel_capabilities(ch: &ChannelRecord) -> ProviderCapabilities {
    if is_plugin_provider(&ch.provider_type) {
        return plugin_manifest(ch.model_mapping.clone(), &ch.base_url)
            .map(|manifest| manifest.capabilities)
            .unwrap_or_else(|_| provider_capabilities(&ch.provider_type));
    }
    provider_capabilities(&ch.provider_type)
}

fn is_plugin_provider(provider_type: &str) -> bool {
    matches!(provider_type, "plugin" | "custom" | "http" | "http_plugin")
}
