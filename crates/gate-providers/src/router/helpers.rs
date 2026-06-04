//! Stateless helpers — model fallback chain、model mapping resolve、provider_type 谓词、env secret map、secret slot 归一化。

use crate::custom_provider::CustomHttpProvider;
use crate::plugin_manifest::plugin_manifest;
use crate::{ProviderCapabilities, provider_capabilities};
use std::collections::HashMap;

/// 给某 model 提供 fallback chain。
///
/// 仅在主路由返回 None 时按顺序尝试。
pub(super) fn fallback_models(model: &str) -> &'static [&'static str] {
    match model {
        "gpt-4o" => &["gpt-4o-mini"],
        "claude-3-opus" => &["claude-3-sonnet", "claude-3-haiku"],
        "claude-3-sonnet" => &["claude-3-haiku"],
        "gemini-1.5-pro" => &["gemini-1.5-flash"],
        _ => &[],
    }
}

pub(super) fn resolve_model_mapping(mapping: &serde_json::Value, model: &str) -> String {
    let mapping = mapping
        .as_object()
        .and_then(|map| {
            if map.contains_key("plugin") {
                map.get("models")
                    .or_else(|| map.get("model_aliases"))
                    .or_else(|| map.get("deployments"))
            } else {
                Some(mapping)
            }
        })
        .unwrap_or(mapping);
    if let serde_json::Value::Object(map) = mapping
        && let Some(serde_json::Value::String(target)) = map.get(model)
    {
        return target.clone();
    }
    model.to_string()
}

pub(super) fn is_plugin_provider(provider_type: &str) -> bool {
    matches!(provider_type, "plugin" | "custom" | "http" | "http_plugin")
}

/// ADR-0005：provider_type 是否落在 native 命名空间（`native:<name>`）。
pub(super) fn is_native_provider(provider_type: &str) -> bool {
    crate::native::is_native_provider_type(provider_type)
}

pub(super) fn supports_image_runtime(provider_type: &str) -> bool {
    matches!(provider_type, "openai" | "openai_compatible")
}

pub(super) fn supports_audio_runtime(provider_type: &str) -> bool {
    matches!(provider_type, "openai" | "openai_compatible")
}

pub(super) fn channel_capabilities(channel: &gate_storage::ChannelRecord) -> ProviderCapabilities {
    if is_plugin_provider(&channel.provider_type) {
        return plugin_manifest(channel.model_mapping.clone(), &channel.base_url)
            .map(|manifest| manifest.capabilities)
            .unwrap_or_else(|_| provider_capabilities(&channel.provider_type));
    }
    // ADR-0005：native 渠道自报 capabilities，路由层无需 hardcode if provider=="kiro"。
    if let Some(name) = crate::native::native_name(&channel.provider_type) {
        return crate::native::native_provider_capabilities(name)
            .unwrap_or_else(ProviderCapabilities::chat_stream);
    }
    provider_capabilities(&channel.provider_type)
}

pub(super) fn env_secret_map(channel_code: &str, primary: String) -> HashMap<String, String> {
    let mut secrets = CustomHttpProvider::env_secret_slots(channel_code);
    secrets.insert("primary".to_string(), primary);
    secrets
}

pub(super) fn normalize_secret_slot(slot: &str) -> String {
    let trimmed = slot.trim();
    if trimmed.is_empty() || trimmed == "api_key" {
        "primary".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    }
}
