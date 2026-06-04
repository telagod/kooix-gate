//! Provider builders — 按 channel.provider_type 构造对应的 Provider/Embedding/Image/Audio。
//!
//! v0.5.0-rc2（ADR-0004）起：4 大编译期 wrapper（openai/anthropic/azure/bedrock）已删，
//! 全部走 plugin runtime + builtin_fastpath 静态分发（ADR-0002）。本文件只剩 plugin
//! 接入面 + legacy provider_type fail-loud。

use super::helpers::env_secret_map;
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
    // ADR-0005 native plane：provider_type='native:<name>' 走命令式渠道层。
    // manifest 表达不了的重渠道（kiro/windsurf）在此构造，与下面的声明式
    // plugin 分支并存。secrets/opts move 进 ctx；非 native 时不消费，留给 match。
    if let Some(name) = crate::native::native_name(&channel.provider_type) {
        let ctx = crate::native::NativeBuildContext {
            channel,
            secrets,
            opts,
        };
        return crate::native::build_native_provider(name, &ctx);
    }
    match channel.provider_type.as_str() {
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
        // 0.3.0 起 cohere/deepseek/gemini/mistral/ollama 已迁移到 plugin runtime
        // （migration 20260522000001）。
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
        // v0.5.0-rc2 起 openai/anthropic/azure/bedrock 已迁移到 plugin runtime + fastpath
        // （migration 20260528000001 + ADR-0004）。
        legacy @ ("openai" | "anthropic" | "azure" | "bedrock") => {
            let preset = match legacy {
                "openai" => "openai",
                "anthropic" => "anthropic_messages",
                "azure" => "azure_openai",
                "bedrock" => "bedrock_converse",
                _ => unreachable!(),
            };
            Err(ProviderError::Config(format!(
                "channel '{}' has provider_type='{}' which was retired in v0.5.0-rc2 (ADR-0004); \
                 run `kgctl migrate` to convert it to a plugin preset, \
                 or manually set provider_type='plugin' with model_mapping.plugin.preset.provider='{}'",
                channel.code, legacy, preset
            )))
        }
        unknown => Err(ProviderError::Config(format!(
            "channel '{}' has unknown provider_type='{}'; \
             use provider_type='plugin' with model_mapping.plugin.preset.provider=<preset-name>",
            channel.code, unknown
        ))),
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
    _api_key: String,
    secrets: HashMap<String, String>,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn EmbeddingProvider>> {
    match channel.provider_type.as_str() {
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
        legacy @ ("cohere" | "deepseek" | "gemini" | "mistral" | "ollama") => {
            Err(ProviderError::Config(format!(
                "channel '{}' has provider_type='{}' which was retired in 0.3.0 for embeddings; \
                 run `kgctl migrate` or set provider_type='plugin'",
                channel.code, legacy
            )))
        }
        legacy @ ("openai" | "anthropic" | "azure" | "bedrock") => {
            Err(ProviderError::Config(format!(
                "channel '{}' has provider_type='{}' which was retired in v0.5.0-rc2; \
                 run `kgctl migrate` or set provider_type='plugin'",
                channel.code, legacy
            )))
        }
        unknown => Err(ProviderError::Config(format!(
            "channel '{}' has unknown provider_type='{}' for embeddings; \
             use provider_type='plugin' with model_mapping.plugin.preset.provider=<preset-name>",
            channel.code, unknown
        ))),
    }
}

/// 按 channel 构造 ImageProvider 实例。
///
/// v0.5.0-rc2（ADR-0004）起：plugin manifest 暂未支持 image generations endpoint
/// 配置，所有 image channel 默认走 OpenAI 兼容路径（`{base_url}/images/generations`）。
/// 用户通过 plugin manifest 配置非 OpenAI 兼容的 image endpoint 需要后续 ADR。
pub(super) fn build_image_provider(
    channel: &gate_storage::ChannelRecord,
    api_key: String,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn ImageProvider>> {
    let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
        .map_err(|e| ProviderError::Config(format!("build image provider: {e}")))?;
    Ok(Arc::new(p) as Arc<dyn ImageProvider>)
}

/// 按 channel 构造 AudioProvider 实例。
///
/// v0.5.0-rc2（ADR-0004）起：plugin manifest 暂未支持 audio speech/transcription
/// endpoint 配置，所有 audio channel 默认走 OpenAI 兼容路径。
pub(super) fn build_audio_provider(
    channel: &gate_storage::ChannelRecord,
    api_key: String,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn AudioProvider>> {
    let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
        .map_err(|e| ProviderError::Config(format!("build audio provider: {e}")))?;
    Ok(Arc::new(p) as Arc<dyn AudioProvider>)
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
