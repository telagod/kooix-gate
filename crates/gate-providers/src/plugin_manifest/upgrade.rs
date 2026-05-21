//! Plugin manifest v0 → v1 upgrade path。
//!
//! - `extract_plugin_value`：从 `model_mapping` JSON 中提取 plugin 子结构。
//! - `deserialize_v1`：直接按 v1 schema 解析。
//! - `upgrade_v0`：识别 legacy v0 字段（preset / request.chat_path 等），重塑为 v1。

use super::*;
use crate::error::{ProviderError, ProviderResult};

pub(super) fn extract_plugin_value(value: Value) -> (Value, String) {
    if let Some(v) = value.get("plugin") {
        (v.clone(), "/plugin".to_string())
    } else if let Some(v) = value.get("adapter") {
        (v.clone(), "/adapter".to_string())
    } else if let Some(v) = value.get("protocol") {
        (v.clone(), "/protocol".to_string())
    } else {
        (value, String::new())
    }
}

pub(super) fn deserialize_v1(value: Value, pointer_base: &str) -> ProviderResult<PluginManifest> {
    let input = value.to_string();
    let mut de = serde_json::Deserializer::from_str(&input);
    serde_path_to_error::deserialize::<_, PluginManifest>(&mut de).map_err(|err| {
        let pointer = json_pointer(pointer_base, &path_to_json_pointer(err.path()));
        ProviderError::Config(format!(
            "invalid plugin manifest at {pointer}: {}",
            err.inner()
        ))
    })
}

pub(super) fn upgrade_v0(value: Value, pointer_base: &str) -> ProviderResult<PluginManifest> {
    let input = value.to_string();
    let mut de = serde_json::Deserializer::from_str(&input);
    let legacy =
        serde_path_to_error::deserialize::<_, LegacyPluginManifest>(&mut de).map_err(|err| {
            let pointer = json_pointer(pointer_base, &path_to_json_pointer(err.path()));
            ProviderError::Config(format!(
                "invalid legacy plugin manifest at {pointer}: {}",
                err.inner()
            ))
        })?;

    Ok(PluginManifest {
        preset: legacy.preset,
        request: RequestManifest {
            path: legacy.request.chat_path,
            embedding_path: legacy
                .request
                .embedding_path
                .or(legacy.request.embeddings_path),
            headers: legacy.request.headers,
            body: legacy.request.body,
            embedding_body: legacy.request.embedding_body,
            retry: legacy.request.retry,
            force_stream_field: legacy.request.force_stream_field,
            stream_path: legacy.request.stream_path,
            ..Default::default()
        },
        response: legacy.response,
        embedding_response: legacy.embedding_response,
        stream: legacy.stream,
        error: legacy.error,
        probe: legacy.probe,
        security: legacy.security,
        ..Default::default()
    })
}

impl Default for LegacyRequestManifest {
    fn default() -> Self {
        Self {
            chat_path: None,
            embedding_path: None,
            embeddings_path: None,
            headers: Map::new(),
            body: None,
            embedding_body: None,
            retry: RetryManifest::default(),
            force_stream_field: true,
            stream_path: "stream".to_string(),
        }
    }
}
