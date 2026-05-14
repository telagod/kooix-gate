//! POST /v1/audio/speech — TTS 代理（OpenAI 兼容）。
//! POST /v1/audio/transcriptions — STT 代理（OpenAI 兼容）。

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Multipart, State};
use axum::http::header;
use axum::response::Response;
use axum::{Json, Router, routing::post};
use gate_providers::types::{AudioSpeechRequest, AudioTranscriptionResponse};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/audio/speech", post(create_speech))
        .route("/audio/transcriptions", post(create_transcription))
}

async fn create_speech(
    State(app): State<AppState>,
    Authed(_ctx): Authed,
    Json(req): Json<AudioSpeechRequest>,
) -> AppResult<Response> {
    let provider = app
        .audio_provider
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("TTS not configured".into()))?;

    let format = req.response_format.clone().unwrap_or_else(|| "mp3".to_string());
    let content_type = match format.as_str() {
        "opus" => "audio/opus",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "pcm" => "audio/pcm",
        _ => "audio/mpeg",
    };

    let audio_bytes = provider
        .speech(req)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(audio_bytes))
        .unwrap())
}

async fn create_transcription(
    State(app): State<AppState>,
    Authed(_ctx): Authed,
    mut multipart: Multipart,
) -> AppResult<Json<AudioTranscriptionResponse>> {
    let provider = app
        .audio_provider
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("STT not configured".into()))?;

    let mut audio_data: Option<bytes::Bytes> = None;
    let mut filename = "audio.wav".to_string();
    let mut model = "whisper-1".to_string();
    let mut language: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::BadRequest(e.to_string()))? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                if let Some(fname) = field.file_name() {
                    filename = fname.to_string();
                }
                let data = field.bytes().await.map_err(|e| AppError::BadRequest(e.to_string()))?;
                audio_data = Some(data);
            }
            "model" => {
                model = field.text().await.unwrap_or_else(|_| "whisper-1".to_string());
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

    let audio_bytes = bytes::Bytes::from(audio.to_vec());

    let resp = provider
        .transcription(audio_bytes, filename, model, language)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(resp))
}
