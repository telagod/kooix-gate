//! plugin_manifest 公共入口函数 — 校验 / 解析 / retry 提取 / JSON Schema。

use super::*;
use crate::error::ProviderResult;

pub fn validate_plugin_manifest(value: Value, base_url: &str) -> ProviderResult<()> {
    PluginManifest::from_value(value, base_url).map(|_| ())
}

pub fn plugin_manifest(value: Value, base_url: &str) -> ProviderResult<PluginManifest> {
    PluginManifest::from_value(value, base_url)
}

/// v0.5.0-rc2（ADR-0004）：暴露 manifest 要求的 secret slot 列表，供 router
/// 在 `has_available_plugin_secret` 中精确判断 env fallback 是否能满足所有要求的 slot。
///
/// 解析失败返回空 vec（caller 应继续按 channel.code env 兜底）。
pub fn manifest_required_secret_slots(value: Value, base_url: &str) -> Vec<String> {
    PluginManifest::from_value(value, base_url)
        .map(|m| super::validate::required_secret_slots(&m.auth))
        .unwrap_or_default()
}

pub fn plugin_manifest_retry_config(
    value: &Value,
    base_url: &str,
) -> ProviderResult<crate::retry::RetryConfig> {
    let manifest = PluginManifest::from_value(value.clone(), base_url)?;
    let mut config = crate::retry::RetryConfig::default();
    if let Some(max_retries) = manifest.request.retry.max_retries {
        config.max_retries = max_retries as u32;
    }
    for status in manifest
        .request
        .retry
        .retryable_status
        .iter()
        .chain(manifest.error.retryable_status.iter())
    {
        if !config.retryable_status_codes.contains(status) {
            config.retryable_status_codes.push(*status);
        }
    }
    for code in manifest
        .request
        .retry
        .retryable_codes
        .iter()
        .chain(manifest.error.retryable_codes.iter())
    {
        if !config
            .retryable_error_codes
            .iter()
            .any(|existing| existing == code)
        {
            config.retryable_error_codes.push(code.clone());
        }
    }
    if let Some(cooldown_ms) = manifest
        .request
        .retry
        .cooldown_ms
        .or(manifest.error.cooldown_ms)
    {
        config.max_backoff_ms = config.max_backoff_ms.max(cooldown_ms);
    }
    Ok(config)
}

pub fn plugin_manifest_schema_json() -> Value {
    let mut schema = schemars::schema_for!(ChannelPluginMapping).to_value();
    if let Some(obj) = schema.as_object_mut() {
        obj.insert(
            "$id".to_string(),
            json!("https://kooix-gate.local/schemas/plugin-manifest-v1.json"),
        );
        obj.insert(
            "title".to_string(),
            json!("Kooix Gate HTTP Plugin Manifest v1"),
        );
        obj.insert(
            "description".to_string(),
            json!("Schema for channels.model_mapping.plugin. Runtime also accepts legacy v0 manifests and upgrades them to plugin.version=1."),
        );
    }
    schema
}
