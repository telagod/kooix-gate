//! Provider builders — 按 channel.provider_type 构造对应的 Provider/Embedding/Image/Audio。
//!
//! 0.3.0 起：`gemini`/`deepseek`/`ollama`/`mistral`/`cohere` 这 5 个编译期 thin wrapper
//! 已删除（migration 20260522000001 自动改成 plugin + preset）。本文件只保留 4 个
//! fast-path 编译期 provider（OpenAI / Anthropic / Azure / Bedrock）；其余全部走 plugin runtime。
//! 见 ADR-0001（docs/architecture/decisions/ADR-0001-providers-as-plugin.md）。

use super::helpers::env_secret_map;
use crate::anthropic::AnthropicProvider;
use crate::azure::AzureProvider;
use crate::bedrock::BedrockProvider;
use crate::custom_provider::CustomHttpProvider;
use crate::error::{ProviderError, ProviderResult};
use crate::openai::OpenAiProvider;
use crate::{AudioProvider, EmbeddingProvider, ImageProvider, Provider};
use std::collections::HashMap;
use std::sync::Arc;

/// 按 provider_type 构造 Provider 实例。
pub(super) fn build_provider(
    channel: &gate_storage::ChannelRecord,
    api_key: String,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn Provider>> {
    build_provider_with_secrets(
        channel,
        HashMap::from([("primary".to_string(), api_key)]),
        opts,
    )
}

/// 按 provider_type 构造 Provider 实例，并把 manifest secret slots 传入 runtime plugin。
pub(super) fn build_provider_with_secrets(
    channel: &gate_storage::ChannelRecord,
    secrets: HashMap<String, String>,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn Provider>> {
    let api_key = secrets.get("primary").cloned().unwrap_or_default();
    match channel.provider_type.as_str() {
        "anthropic" => {
            let p = AnthropicProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build AnthropicProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        "azure" => {
            let p = AzureProvider::new_with_opts(channel.base_url.clone(), api_key, None, opts)
                .map_err(|e| ProviderError::Config(format!("build AzureProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        "bedrock" => {
            let access = api_key;
            let secret_env = format!(
                "KOOIX_CH_{}_SECRET",
                channel
                    .code
                    .to_uppercase()
                    .replace(|c: char| !c.is_alphanumeric(), "_")
            );
            let secret = std::env::var(&secret_env).map_err(|_| {
                ProviderError::Config(format!(
                    "missing {} env var for bedrock channel '{}'",
                    secret_env, channel.code
                ))
            })?;
            let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
            let p = BedrockProvider::new_with_opts(region, access, secret, opts)
                .map_err(|e| ProviderError::Config(format!("build BedrockProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        "plugin" | "custom" | "http" | "http_plugin" => {
            let p = CustomHttpProvider::new_with_secret_slots(
                channel.base_url.clone(),
                secrets,
                channel.model_mapping.clone(),
                opts,
            )
            .map_err(|e| ProviderError::Config(format!("build CustomHttpProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        // 0.3.0 起 cohere/deepseek/gemini/mistral/ollama 已迁移到 plugin runtime（migration
        // 20260522000001 自动改 provider_type）。如果到了这里说明 channel migration 没跑或被
        // 手工绕过，让用户清晰看到错误而非静默走 OpenAI 兼容回退。
        legacy @ ("cohere" | "deepseek" | "gemini" | "mistral" | "ollama") => {
            Err(ProviderError::Config(format!(
                "channel '{}' has provider_type='{}' which was retired in 0.3.0; \
                 run `kgctl migrate` to convert it to a plugin preset, \
                 or manually set provider_type='plugin' with model_mapping.plugin.preset.provider='{}'",
                channel.code,
                legacy,
                if legacy == "cohere" {
                    "cohere_chat"
                } else {
                    legacy
                }
            )))
        }
        _ => {
            // 未知类型走 OpenAI 兼容
            let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
    }
}

/// 按 provider_type 构造 EmbeddingProvider 实例。
pub(super) fn build_embedding_provider(
    channel: &gate_storage::ChannelRecord,
    api_key: String,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn EmbeddingProvider>> {
    build_embedding_provider_with_secrets(
        channel,
        api_key.clone(),
        env_secret_map(&channel.code, api_key),
        opts,
    )
}

pub(super) fn build_embedding_provider_with_secrets(
    channel: &gate_storage::ChannelRecord,
    api_key: String,
    secrets: HashMap<String, String>,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn EmbeddingProvider>> {
    match channel.provider_type.as_str() {
        "azure" => {
            let p = AzureProvider::new_with_opts(channel.base_url.clone(), api_key, None, opts)
                .map_err(|e| ProviderError::Config(format!("build AzureProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn EmbeddingProvider>)
        }
        "plugin" | "custom" | "http" | "http_plugin" => {
            let p = CustomHttpProvider::new_with_secret_slots(
                channel.base_url.clone(),
                secrets,
                channel.model_mapping.clone(),
                opts,
            )
            .map_err(|e| ProviderError::Config(format!("build CustomHttpProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn EmbeddingProvider>)
        }
        // 0.3.0 retired thin wrappers — see migration 20260522000001.
        legacy @ ("cohere" | "deepseek" | "gemini" | "mistral" | "ollama") => {
            Err(ProviderError::Config(format!(
                "channel '{}' has provider_type='{}' which was retired in 0.3.0 for embeddings; \
                 run `kgctl migrate` or set provider_type='plugin'",
                channel.code, legacy
            )))
        }
        _ => {
            let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn EmbeddingProvider>)
        }
    }
}

/// 按 provider_type 构造 ImageProvider 实例。
pub(super) fn build_image_provider(
    channel: &gate_storage::ChannelRecord,
    api_key: String,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn ImageProvider>> {
    match channel.provider_type.as_str() {
        "openai" | "openai_compatible" => {
            let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn ImageProvider>)
        }
        _ => {
            let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn ImageProvider>)
        }
    }
}

/// 按 provider_type 构造 AudioProvider 实例。
pub(super) fn build_audio_provider(
    channel: &gate_storage::ChannelRecord,
    api_key: String,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn AudioProvider>> {
    match channel.provider_type.as_str() {
        "openai" | "openai_compatible" => {
            let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn AudioProvider>)
        }
        _ => {
            let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn AudioProvider>)
        }
    }
}

/// API key 来源策略（env 回退，DB 优先路径在 route_for_model 内）。
///
/// 优先级：
/// 1. 环境变量 `KOOIX_CH_<CODE>_KEY`（code 大写，非字母替换为 _）
/// 2. 环境变量 `KOOIX_API_KEY`（全局兜底）
/// 3. 空字符串（plugin auth 可完全依赖 named secret slots；上游自己决定是否拒绝）
pub(super) fn resolve_api_key_for_channel(code: &str) -> ProviderResult<String> {
    let env_key = format!(
        "KOOIX_CH_{}_KEY",
        code.to_uppercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    Ok(std::env::var(&env_key)
        .or_else(|_| std::env::var("KOOIX_API_KEY"))
        .unwrap_or_default())
}
