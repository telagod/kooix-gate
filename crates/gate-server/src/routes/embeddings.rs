//! POST /v1/embeddings — Embedding 代理。
//!
//! 路由逻辑与 chat 类似：找到支持该 model 的 channel → 构建 EmbeddingProvider → 调用。

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::State;
use axum::{Json, Router, routing::post};
use gate_providers::types::{EmbeddingRequest, EmbeddingResponse};
use gate_providers::{EmbeddingProvider, openai::OpenAiProvider};

pub fn router() -> Router<AppState> {
    Router::new().route("/embeddings", post(create_embedding))
}

async fn create_embedding(
    State(app): State<AppState>,
    Authed(_ctx): Authed,
    Json(req): Json<EmbeddingRequest>,
) -> AppResult<Json<EmbeddingResponse>> {
    let channels = app.repos.channels.list_admin_view().await?;
    let matching = channels.iter().find(|ch| {
        ch.status == "active"
            && (ch.supported_models.is_empty()
                || ch.supported_models.iter().any(|m| m == &req.model))
    });

    let ch = matching.ok_or_else(|| {
        AppError::BadRequest(format!("no channel supports embedding model '{}'", req.model))
    })?;

    let key_code = ch.code.to_uppercase().replace(|c: char| !c.is_alphanumeric(), "_");
    let api_key = std::env::var(format!("KOOIX_CH_{key_code}_KEY"))
        .or_else(|_| std::env::var("KOOIX_API_KEY"))
        .unwrap_or_default();

    let provider: Box<dyn EmbeddingProvider> = match ch.provider_type.as_str() {
        "cohere" => Box::new(
            gate_providers::cohere::CohereProvider::new(&ch.base_url, &api_key)
                .map_err(|e| AppError::Internal(e.to_string()))?,
        ),
        "ollama" => Box::new(
            gate_providers::ollama::OllamaProvider::new(&ch.base_url)
                .map_err(|e| AppError::Internal(e.to_string()))?,
        ),
        _ => Box::new(
            OpenAiProvider::new(&ch.base_url, &api_key)
                .map_err(|e| AppError::Internal(e.to_string()))?,
        ),
    };

    let resp = provider.embed(req).await.map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(resp))
}
