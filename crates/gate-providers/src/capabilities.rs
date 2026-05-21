//! Provider capability matrix used by routing, billing, admin API and UI docs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const VERTEX_OPENAI_BASE_URL: &str = "https://aiplatform.googleapis.com/v1/projects/<project>/locations/<location>/endpoints/openapi";

/// Capability flags shared by compile-time providers and runtime plugin manifests.
///
/// Field names intentionally match `plugin.capabilities` in manifest v1 so API
/// responses can expose one stable shape.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(default)]
pub struct ProviderCapabilities {
    pub chat: bool,
    pub streaming: bool,
    pub tools: bool,
    pub embeddings: bool,
    pub image: bool,
    pub audio: bool,
    pub vision: bool,
    pub json_mode: bool,
    pub batch: bool,
}

impl ProviderCapabilities {
    pub const fn none() -> Self {
        Self {
            chat: false,
            streaming: false,
            tools: false,
            embeddings: false,
            image: false,
            audio: false,
            vision: false,
            json_mode: false,
            batch: false,
        }
    }

    pub const fn chat() -> Self {
        Self {
            chat: true,
            ..Self::none()
        }
    }

    pub const fn chat_stream() -> Self {
        Self {
            chat: true,
            streaming: true,
            ..Self::none()
        }
    }

    pub const fn openai_compatible_core() -> Self {
        Self {
            chat: true,
            streaming: true,
            tools: true,
            embeddings: true,
            vision: true,
            json_mode: true,
            ..Self::none()
        }
    }

    pub const fn openai_full() -> Self {
        Self {
            image: true,
            audio: true,
            ..Self::openai_compatible_core()
        }
    }

    pub fn merge_truthy_defaults(&mut self, defaults: &Self) {
        self.chat |= defaults.chat;
        self.streaming |= defaults.streaming;
        self.tools |= defaults.tools;
        self.embeddings |= defaults.embeddings;
        self.image |= defaults.image;
        self.audio |= defaults.audio;
        self.vision |= defaults.vision;
        self.json_mode |= defaults.json_mode;
        self.batch |= defaults.batch;
    }

    /// Merge another capability set by OR-ing truthy fields.
    ///
    /// Used when one public model is served by multiple channels and the API
    /// needs to expose the union of currently available runtime capabilities.
    pub fn merge_truthy(&mut self, other: &Self) {
        self.merge_truthy_defaults(other);
    }

    pub fn missing_for_chat_request(
        &self,
        req: &crate::types::ChatRequest,
    ) -> Vec<ProviderCapability> {
        let mut missing = Vec::new();
        if !self.chat {
            missing.push(ProviderCapability::Chat);
        }
        if req.stream && !self.streaming {
            missing.push(ProviderCapability::Streaming);
        }
        if req.tools.as_ref().is_some_and(|tools| !tools.is_empty()) && !self.tools {
            missing.push(ProviderCapability::Tools);
        }
        if req
            .messages
            .iter()
            .any(|message| matches!(&message.content, Some(crate::types::MessageContent::Parts(parts)) if parts.iter().any(|part| matches!(part, crate::types::ContentPart::ImageUrl { .. }))))
            && !self.vision
        {
            missing.push(ProviderCapability::Vision);
        }
        if req.extra.contains_key("response_format") && !self.json_mode {
            missing.push(ProviderCapability::JsonMode);
        }
        missing
    }
}

/// Individual capability names for runtime decisions and precise diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderCapability {
    Chat,
    Streaming,
    Tools,
    Embeddings,
    Image,
    Audio,
    Vision,
    JsonMode,
    Batch,
}

impl ProviderCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            ProviderCapability::Chat => "chat",
            ProviderCapability::Streaming => "streaming",
            ProviderCapability::Tools => "tools",
            ProviderCapability::Embeddings => "embeddings",
            ProviderCapability::Image => "image",
            ProviderCapability::Audio => "audio",
            ProviderCapability::Vision => "vision",
            ProviderCapability::JsonMode => "json_mode",
            ProviderCapability::Batch => "batch",
        }
    }
}

/// Compile-time provider capability defaults.
pub fn provider_capabilities(provider_type: &str) -> ProviderCapabilities {
    match normalize_provider_type(provider_type).as_str() {
        "openai" => ProviderCapabilities::openai_full(),
        "azure" | "vertex" => ProviderCapabilities::openai_compatible_core(),
        "anthropic" => ProviderCapabilities {
            chat: true,
            streaming: true,
            tools: true,
            vision: true,
            json_mode: true,
            ..ProviderCapabilities::none()
        },
        "gemini" => ProviderCapabilities::openai_compatible_core(),
        "deepseek" => ProviderCapabilities {
            chat: true,
            streaming: true,
            json_mode: true,
            ..ProviderCapabilities::none()
        },
        "ollama" => ProviderCapabilities {
            chat: true,
            streaming: true,
            embeddings: true,
            tools: true,
            vision: true,
            json_mode: true,
            ..ProviderCapabilities::none()
        },
        "mistral" => ProviderCapabilities::openai_compatible_core(),
        "cohere" => ProviderCapabilities {
            chat: true,
            streaming: true,
            tools: true,
            embeddings: true,
            json_mode: true,
            ..ProviderCapabilities::none()
        },
        "bedrock" => ProviderCapabilities {
            chat: true,
            streaming: true,
            ..ProviderCapabilities::none()
        },
        "plugin" | "custom" | "http" | "http_plugin" => ProviderCapabilities::chat_stream(),
        // The server currently builds unknown provider_type values through the OpenAI-compatible path.
        _ => ProviderCapabilities::openai_compatible_core(),
    }
}

/// Suggested base URLs for compile-time providers.
pub fn provider_base_url_suggestion(provider_type: &str) -> Option<&'static str> {
    match normalize_provider_type(provider_type).as_str() {
        "openai" => Some("https://api.openai.com/v1"),
        "anthropic" => Some("https://api.anthropic.com"),
        "gemini" => Some("https://generativelanguage.googleapis.com"),
        "azure" => Some("https://<resource>.openai.azure.com"),
        "vertex" => Some(VERTEX_OPENAI_BASE_URL),
        "bedrock" => Some("https://bedrock-runtime.<region>.amazonaws.com"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "ollama" => Some("http://localhost:11434/v1"),
        "mistral" => Some("https://api.mistral.ai/v1"),
        "cohere" => Some("https://api.cohere.com/v2"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "together" => Some("https://api.together.xyz/v1"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "moonshot" => Some("https://api.moonshot.cn/v1"),
        "zhipu" => Some("https://open.bigmodel.cn/api/paas/v4"),
        "qwen" => Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
        "yi" => Some("https://api.lingyiwanwu.com/v1"),
        "plugin" | "custom" | "http" | "http_plugin" => Some("https://api.example.com/v1"),
        _ => None,
    }
}

/// Runtime plugin preset capability defaults.
pub fn plugin_preset_capabilities(provider: &str) -> Option<ProviderCapabilities> {
    match normalize_provider_type(provider).as_str() {
        "openai" | "openai_compatible" | "vllm" | "lm_studio" | "ollama_openai" | "localai"
        | "xinference" | "vertex_openai" | "groq" | "together" | "openrouter" | "moonshot"
        | "zhipu" | "qwen" | "yi" => Some(ProviderCapabilities::openai_compatible_core()),
        "deepseek" => Some(ProviderCapabilities {
            chat: true,
            streaming: true,
            json_mode: true,
            ..ProviderCapabilities::none()
        }),
        "mistral" | "gemini" => Some(ProviderCapabilities::openai_compatible_core()),
        "azure_openai" => Some(ProviderCapabilities::openai_compatible_core()),
        "ollama" => Some(ProviderCapabilities {
            chat: true,
            streaming: true,
            embeddings: true,
            tools: true,
            vision: true,
            json_mode: true,
            ..ProviderCapabilities::none()
        }),
        "cohere_chat" => Some(ProviderCapabilities {
            chat: true,
            streaming: true,
            tools: true,
            embeddings: true,
            json_mode: true,
            ..ProviderCapabilities::none()
        }),
        "anthropic_messages" => Some(ProviderCapabilities {
            chat: true,
            streaming: true,
            tools: true,
            vision: true,
            json_mode: true,
            ..ProviderCapabilities::none()
        }),
        "bedrock_converse" => Some(ProviderCapabilities {
            chat: true,
            streaming: true,
            ..ProviderCapabilities::none()
        }),
        _ => None,
    }
}

/// Suggested base URLs for runtime plugin presets.
pub fn plugin_preset_base_url_suggestion(provider: &str) -> Option<&'static str> {
    match normalize_provider_type(provider).as_str() {
        "openai" | "openai_compatible" => Some("https://api.openai.com/v1"),
        "vllm" => Some("http://localhost:8000/v1"),
        "lm_studio" => Some("http://localhost:1234/v1"),
        "ollama" | "ollama_openai" => Some("http://localhost:11434/v1"),
        "localai" => Some("http://localhost:8080/v1"),
        "xinference" => Some("http://localhost:9997/v1"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "mistral" => Some("https://api.mistral.ai/v1"),
        "gemini" => Some("https://generativelanguage.googleapis.com"),
        "azure_openai" => Some("https://<resource>.openai.azure.com"),
        "vertex_openai" => Some(VERTEX_OPENAI_BASE_URL),
        "anthropic_messages" => Some("https://api.anthropic.com"),
        "bedrock_converse" => Some("https://bedrock-runtime.<region>.amazonaws.com"),
        "cohere_chat" => Some("https://api.cohere.com/v2"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "together" => Some("https://api.together.xyz/v1"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "moonshot" => Some("https://api.moonshot.cn/v1"),
        "zhipu" => Some("https://open.bigmodel.cn/api/paas/v4"),
        "qwen" => Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
        "yi" => Some("https://api.lingyiwanwu.com/v1"),
        _ => None,
    }
}

fn normalize_provider_type(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_capability_defaults_cover_compile_time_providers() {
        let openai = provider_capabilities("openai");
        assert!(openai.chat);
        assert!(openai.streaming);
        assert!(openai.embeddings);
        assert!(openai.image);
        assert!(openai.audio);

        let bedrock = provider_capabilities("bedrock");
        assert!(bedrock.chat);
        assert!(bedrock.streaming);
        assert!(!bedrock.tools);
        assert!(!bedrock.embeddings);
    }

    #[test]
    fn plugin_preset_capability_defaults_cover_openai_variants() {
        for provider in [
            "vllm",
            "lm_studio",
            "ollama_openai",
            "localai",
            "xinference",
            "vertex_openai",
        ] {
            let caps = plugin_preset_capabilities(provider).expect(provider);
            assert!(caps.chat, "{provider}");
            assert!(caps.streaming, "{provider}");
            assert!(caps.embeddings, "{provider}");
            assert!(
                plugin_preset_base_url_suggestion(provider).is_some(),
                "{provider}"
            );
        }
    }
}
