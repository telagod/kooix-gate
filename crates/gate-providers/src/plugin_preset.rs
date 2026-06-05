//! Built-in provider presets for the runtime HTTP plugin.

use crate::capabilities::{ProviderCapabilities, plugin_preset_capabilities};
use crate::error::{ProviderError, ProviderResult};
use crate::types::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::borrow::Cow;

const DEFAULT_CHAT_PATH: &str = "/chat/completions";
const DEFAULT_EMBEDDINGS_PATH: &str = "/embeddings";

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderPresetKind {
    Openai,
    OpenaiCompatible,
    Deepseek,
    Mistral,
    Gemini,
    AzureOpenai,
    VertexOpenai,
    AnthropicMessages,
    BedrockConverse,
    CohereChat,
    Groq,
    Together,
    Openrouter,
    Moonshot,
    Zhipu,
    Qwen,
    Yi,
    Ollama,
    Vllm,
    LmStudio,
    OllamaOpenai,
    Localai,
    Xinference,
    Fireworks,
    Perplexity,
    Cerebras,
    Sambanova,
    Hyperbolic,
    CloudflareAi,
    Jina,
    Baichuan,
    Minimax,
    Stepfun,
    Siliconflow,
    Tgi,
    Jan,
    Llamafile,
    Gpt4all,
    TabbyApi,
    Doubao,
    Xai,
    DeepInfra,
    NvidiaNim,
    Replicate,
    Ai21,
    VoyageAi,
    NovitaAi,
    Lambda,
    LeptonAi,
    NebiusAi,
    Hunyuan,
    Spark,
    FriendliAi,
    ChutesAi,
    InfiniAi,
}

pub(crate) fn provider_preset_name(kind: ProviderPresetKind) -> &'static str {
    match kind {
        ProviderPresetKind::Openai => "openai",
        ProviderPresetKind::OpenaiCompatible => "openai_compatible",
        ProviderPresetKind::Deepseek => "deepseek",
        ProviderPresetKind::Mistral => "mistral",
        ProviderPresetKind::Gemini => "gemini",
        ProviderPresetKind::AzureOpenai => "azure_openai",
        ProviderPresetKind::VertexOpenai => "vertex_openai",
        ProviderPresetKind::AnthropicMessages => "anthropic_messages",
        ProviderPresetKind::BedrockConverse => "bedrock_converse",
        ProviderPresetKind::CohereChat => "cohere_chat",
        ProviderPresetKind::Groq => "groq",
        ProviderPresetKind::Together => "together",
        ProviderPresetKind::Openrouter => "openrouter",
        ProviderPresetKind::Moonshot => "moonshot",
        ProviderPresetKind::Zhipu => "zhipu",
        ProviderPresetKind::Qwen => "qwen",
        ProviderPresetKind::Yi => "yi",
        ProviderPresetKind::Ollama => "ollama",
        ProviderPresetKind::Vllm => "vllm",
        ProviderPresetKind::LmStudio => "lm_studio",
        ProviderPresetKind::OllamaOpenai => "ollama_openai",
        ProviderPresetKind::Localai => "localai",
        ProviderPresetKind::Xinference => "xinference",
        ProviderPresetKind::Fireworks => "fireworks",
        ProviderPresetKind::Perplexity => "perplexity",
        ProviderPresetKind::Cerebras => "cerebras",
        ProviderPresetKind::Sambanova => "sambanova",
        ProviderPresetKind::Hyperbolic => "hyperbolic",
        ProviderPresetKind::CloudflareAi => "cloudflare_ai",
        ProviderPresetKind::Jina => "jina",
        ProviderPresetKind::Baichuan => "baichuan",
        ProviderPresetKind::Minimax => "minimax",
        ProviderPresetKind::Stepfun => "stepfun",
        ProviderPresetKind::Siliconflow => "siliconflow",
        ProviderPresetKind::Tgi => "tgi",
        ProviderPresetKind::Jan => "jan",
        ProviderPresetKind::Llamafile => "llamafile",
        ProviderPresetKind::Gpt4all => "gpt4all",
        ProviderPresetKind::TabbyApi => "tabby_api",
        ProviderPresetKind::Doubao => "doubao",
        ProviderPresetKind::Xai => "xai",
        ProviderPresetKind::DeepInfra => "deep_infra",
        ProviderPresetKind::NvidiaNim => "nvidia_nim",
        ProviderPresetKind::Replicate => "replicate",
        ProviderPresetKind::Ai21 => "ai21",
        ProviderPresetKind::VoyageAi => "voyage_ai",
        ProviderPresetKind::NovitaAi => "novita_ai",
        ProviderPresetKind::Lambda => "lambda",
        ProviderPresetKind::LeptonAi => "lepton_ai",
        ProviderPresetKind::NebiusAi => "nebius_ai",
        ProviderPresetKind::Hunyuan => "hunyuan",
        ProviderPresetKind::Spark => "spark",
        ProviderPresetKind::FriendliAi => "friendli_ai",
        ProviderPresetKind::ChutesAi => "chutes_ai",
        ProviderPresetKind::InfiniAi => "infini_ai",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresetAdapter {
    OpenaiCompatible,
    AnthropicMessages,
    BedrockConverse,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub(crate) struct PresetManifest {
    #[serde(rename = "provider", alias = "kind")]
    pub(crate) kind: Option<ProviderPresetKind>,
    pub(crate) api_version: Option<String>,
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) adapter: Option<PresetAdapter>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub(crate) struct ResponseManifest {
    pub(crate) openai_compatible: Option<bool>,
    pub(crate) id_path: Option<String>,
    pub(crate) model_path: Option<String>,
    pub(crate) content_path: Option<String>,
    pub(crate) reasoning_content_path: Option<String>,
    pub(crate) tool_calls_path: Option<String>,
    pub(crate) finish_reason_path: Option<String>,
    pub(crate) request_id_path: Option<String>,
    pub(crate) metadata_path: Option<String>,
    pub(crate) usage: UsageManifest,
}

impl ResponseManifest {
    pub(crate) fn apply_defaults(&mut self, defaults: Self) {
        self.openai_compatible = self.openai_compatible.or(defaults.openai_compatible);
        self.id_path = self.id_path.take().or(defaults.id_path);
        self.model_path = self.model_path.take().or(defaults.model_path);
        self.content_path = self.content_path.take().or(defaults.content_path);
        self.reasoning_content_path = self
            .reasoning_content_path
            .take()
            .or(defaults.reasoning_content_path);
        self.tool_calls_path = self.tool_calls_path.take().or(defaults.tool_calls_path);
        self.finish_reason_path = self
            .finish_reason_path
            .take()
            .or(defaults.finish_reason_path);
        self.request_id_path = self.request_id_path.take().or(defaults.request_id_path);
        self.metadata_path = self.metadata_path.take().or(defaults.metadata_path);
        self.usage.apply_defaults(defaults.usage);
    }

    pub(crate) fn is_openai_compatible(&self) -> bool {
        self.openai_compatible.unwrap_or(true)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub(crate) struct EmbeddingResponseManifest {
    pub(crate) openai_compatible: Option<bool>,
    pub(crate) object_path: Option<String>,
    pub(crate) data_path: Option<String>,
    pub(crate) embedding_path: Option<String>,
    pub(crate) index_path: Option<String>,
    pub(crate) model_path: Option<String>,
    pub(crate) usage: UsageManifest,
}

impl EmbeddingResponseManifest {
    pub(crate) fn apply_defaults(&mut self, defaults: Self) {
        self.openai_compatible = self.openai_compatible.or(defaults.openai_compatible);
        self.object_path = self.object_path.take().or(defaults.object_path);
        self.data_path = self.data_path.take().or(defaults.data_path);
        self.embedding_path = self.embedding_path.take().or(defaults.embedding_path);
        self.index_path = self.index_path.take().or(defaults.index_path);
        self.model_path = self.model_path.take().or(defaults.model_path);
        self.usage.apply_defaults(defaults.usage);
    }

    pub(crate) fn is_openai_compatible(&self) -> bool {
        self.openai_compatible.unwrap_or(true)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub(crate) struct StreamManifest {
    pub(crate) openai_compatible: Option<bool>,
    pub(crate) event_path: Option<String>,
    pub(crate) ignore_events: Vec<String>,
    pub(crate) done_events: Vec<String>,
    pub(crate) done: Vec<String>,
    pub(crate) done_path: Option<String>,
    pub(crate) done_values: Vec<Value>,
    pub(crate) id_path: Option<String>,
    pub(crate) model_path: Option<String>,
    pub(crate) role_path: Option<String>,
    pub(crate) content_path: Option<String>,
    pub(crate) tool_calls_path: Option<String>,
    pub(crate) finish_reason_path: Option<String>,
    pub(crate) usage: UsageManifest,
}

impl StreamManifest {
    pub(crate) fn apply_defaults(&mut self, defaults: Self) {
        self.openai_compatible = self.openai_compatible.or(defaults.openai_compatible);
        self.event_path = self.event_path.take().or(defaults.event_path);
        if self.ignore_events.is_empty() {
            self.ignore_events = defaults.ignore_events;
        }
        if self.done_events.is_empty() {
            self.done_events = defaults.done_events;
        }
        if self.done.is_empty() {
            self.done = defaults.done;
        }
        self.done_path = self.done_path.take().or(defaults.done_path);
        if self.done_values.is_empty() {
            self.done_values = defaults.done_values;
        }
        self.id_path = self.id_path.take().or(defaults.id_path);
        self.model_path = self.model_path.take().or(defaults.model_path);
        self.role_path = self.role_path.take().or(defaults.role_path);
        self.content_path = self.content_path.take().or(defaults.content_path);
        self.tool_calls_path = self.tool_calls_path.take().or(defaults.tool_calls_path);
        self.finish_reason_path = self
            .finish_reason_path
            .take()
            .or(defaults.finish_reason_path);
        self.usage.apply_defaults(defaults.usage);
    }

    pub(crate) fn is_openai_compatible(&self) -> bool {
        self.openai_compatible.unwrap_or(true)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub(crate) struct UsageManifest {
    pub(crate) prompt_tokens_path: Option<String>,
    pub(crate) completion_tokens_path: Option<String>,
    pub(crate) total_tokens_path: Option<String>,
    pub(crate) cached_tokens_path: Option<String>,
    pub(crate) reasoning_tokens_path: Option<String>,
    pub(crate) image_units_path: Option<String>,
    pub(crate) audio_seconds_path: Option<String>,
    pub(crate) raw_path: Option<String>,
    pub(crate) output_only_completion_tokens: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractedUsage {
    pub(crate) usage: Usage,
    pub(crate) completion_present: bool,
    pub(crate) total_present: bool,
}

impl UsageManifest {
    fn apply_defaults(&mut self, defaults: Self) {
        self.prompt_tokens_path = self
            .prompt_tokens_path
            .take()
            .or(defaults.prompt_tokens_path);
        self.completion_tokens_path = self
            .completion_tokens_path
            .take()
            .or(defaults.completion_tokens_path);
        self.total_tokens_path = self.total_tokens_path.take().or(defaults.total_tokens_path);
        self.cached_tokens_path = self
            .cached_tokens_path
            .take()
            .or(defaults.cached_tokens_path);
        self.reasoning_tokens_path = self
            .reasoning_tokens_path
            .take()
            .or(defaults.reasoning_tokens_path);
        self.image_units_path = self.image_units_path.take().or(defaults.image_units_path);
        self.audio_seconds_path = self
            .audio_seconds_path
            .take()
            .or(defaults.audio_seconds_path);
        self.raw_path = self.raw_path.take().or(defaults.raw_path);
        self.output_only_completion_tokens |= defaults.output_only_completion_tokens;
    }

    pub(crate) fn extract(&self, value: &Value) -> ProviderResult<Usage> {
        Ok(self.extract_with_presence(value)?.usage)
    }

    fn extract_with_presence(&self, value: &Value) -> ProviderResult<ExtractedUsage> {
        let prompt = extract_u32(
            value,
            self.prompt_tokens_path.as_deref(),
            "prompt_tokens_path",
        )?
        .unwrap_or_default();
        let completion_raw = extract_u32(
            value,
            self.completion_tokens_path.as_deref(),
            "completion_tokens_path",
        )?;
        let completion = completion_raw.unwrap_or_default();
        let total_raw = extract_u32(
            value,
            self.total_tokens_path.as_deref(),
            "total_tokens_path",
        )?;
        let total = total_raw.unwrap_or_else(|| prompt + completion);
        let cached = extract_u32(
            value,
            self.cached_tokens_path.as_deref(),
            "cached_tokens_path",
        )?
        .unwrap_or_default();
        let reasoning_tokens = extract_u32(
            value,
            self.reasoning_tokens_path.as_deref(),
            "reasoning_tokens_path",
        )?;
        let image_units = extract_u32(value, self.image_units_path.as_deref(), "image_units_path")?;
        let audio_seconds = extract_f64(
            value,
            self.audio_seconds_path.as_deref(),
            "audio_seconds_path",
        )?;
        let raw = self
            .raw_path
            .as_deref()
            .and_then(|p| eval_path_value(value, p).ok().flatten());

        Ok(ExtractedUsage {
            usage: Usage {
                prompt_tokens: prompt,
                completion_tokens: completion,
                total_tokens: total,
                cached_tokens: cached,
                cache_creation_input_tokens: 0,
                reasoning_tokens,
                audio_tokens: None,
                accepted_prediction_tokens: None,
                rejected_prediction_tokens: None,
                image_units,
                audio_seconds,
                raw,
            },
            completion_present: completion_raw.is_some(),
            total_present: total_raw.is_some(),
        })
    }

    pub(crate) fn extract_optional(&self, value: &Value) -> ProviderResult<Option<ExtractedUsage>> {
        let usage = self.extract_with_presence(value)?;
        Ok((usage.usage.prompt_tokens > 0
            || usage.usage.completion_tokens > 0
            || usage.usage.total_tokens > 0
            || usage.usage.cached_tokens > 0
            || usage.usage.reasoning_tokens.unwrap_or_default() > 0
            || usage.usage.image_units.unwrap_or_default() > 0
            || usage.usage.audio_seconds.unwrap_or_default() > 0.0
            || usage.usage.raw.is_some())
        .then_some(usage))
    }

    pub(crate) fn should_emit_stream_usage(
        &self,
        usage: &ExtractedUsage,
        finish_reason: Option<FinishReason>,
    ) -> bool {
        if finish_reason.is_some() || usage.completion_present || usage.total_present {
            return true;
        }
        // Anthropic-style streams often send a prompt-only message_start frame and
        // an output-only final delta. Keep the prompt-only frame internal unless a
        // later completion/finish frame proves it is terminal.
        if self.output_only_completion_tokens {
            return false;
        }
        usage.usage.prompt_tokens > 0
            || usage.usage.cached_tokens > 0
            || usage.usage.reasoning_tokens.unwrap_or_default() > 0
            || usage.usage.image_units.unwrap_or_default() > 0
            || usage.usage.audio_seconds.unwrap_or_default() > 0.0
            || usage.usage.raw.is_some()
    }
}

#[derive(Debug)]
pub(crate) struct ProviderPresetSpec {
    pub(crate) chat_path: String,
    pub(crate) embedding_path: Option<String>,
    pub(crate) headers: Map<String, Value>,
    pub(crate) body: Option<Value>,
    pub(crate) embedding_body: Option<Value>,
    pub(crate) stream_path: Option<String>,
    pub(crate) response: ResponseManifest,
    pub(crate) embedding_response: EmbeddingResponseManifest,
    pub(crate) stream: StreamManifest,
    pub(crate) adapter: Option<PresetAdapter>,
    pub(crate) capabilities: ProviderCapabilities,
}

impl ProviderPresetSpec {
    pub(crate) fn for_kind(
        kind: ProviderPresetKind,
        base_url: &str,
        api_version: Option<&str>,
    ) -> ProviderResult<Self> {
        let spec = match kind {
            ProviderPresetKind::Openai
            | ProviderPresetKind::OpenaiCompatible
            | ProviderPresetKind::Deepseek
            | ProviderPresetKind::Mistral
            | ProviderPresetKind::Groq
            | ProviderPresetKind::Together
            | ProviderPresetKind::Openrouter
            | ProviderPresetKind::Moonshot
            | ProviderPresetKind::Zhipu
            | ProviderPresetKind::Qwen
            | ProviderPresetKind::Yi
            | ProviderPresetKind::Ollama
            | ProviderPresetKind::Vllm
            | ProviderPresetKind::LmStudio
            | ProviderPresetKind::OllamaOpenai
            | ProviderPresetKind::Localai
            | ProviderPresetKind::Xinference
            | ProviderPresetKind::VertexOpenai
            | ProviderPresetKind::Fireworks
            | ProviderPresetKind::Perplexity
            | ProviderPresetKind::Cerebras
            | ProviderPresetKind::Sambanova
            | ProviderPresetKind::Hyperbolic
            | ProviderPresetKind::CloudflareAi
            | ProviderPresetKind::Jina
            | ProviderPresetKind::Baichuan
            | ProviderPresetKind::Minimax
            | ProviderPresetKind::Stepfun
            | ProviderPresetKind::Siliconflow
            | ProviderPresetKind::Tgi
            | ProviderPresetKind::Jan
            | ProviderPresetKind::Llamafile
            | ProviderPresetKind::Gpt4all
            | ProviderPresetKind::TabbyApi
            | ProviderPresetKind::Doubao
            | ProviderPresetKind::Xai
            | ProviderPresetKind::DeepInfra
            | ProviderPresetKind::NvidiaNim
            | ProviderPresetKind::Replicate
            | ProviderPresetKind::Ai21
            | ProviderPresetKind::VoyageAi
            | ProviderPresetKind::NovitaAi
            | ProviderPresetKind::Lambda
            | ProviderPresetKind::LeptonAi
            | ProviderPresetKind::NebiusAi
            | ProviderPresetKind::Hunyuan
            | ProviderPresetKind::Spark
            | ProviderPresetKind::FriendliAi
            | ProviderPresetKind::ChutesAi
            | ProviderPresetKind::InfiniAi => Self::openai_compatible(DEFAULT_CHAT_PATH),
            ProviderPresetKind::Gemini => {
                Self::openai_compatible("/v1beta/openai/chat/completions")
                    .with_embedding_path("/v1beta/openai/embeddings")
            }
            ProviderPresetKind::AzureOpenai => Self::openai_compatible(format!(
                "/openai/deployments/{{{{model}}}}/chat/completions?api-version={}",
                api_version.unwrap_or("2024-08-01-preview")
            ))
            .with_embedding_path(format!(
                "/openai/deployments/{{{{model}}}}/embeddings?api-version={}",
                api_version.unwrap_or("2024-08-01-preview")
            ))
            .with_header("api-key", json!("{{api_key}}"))
            .without_bearer(),
            ProviderPresetKind::AnthropicMessages => Self::anthropic_messages(),
            ProviderPresetKind::BedrockConverse => Self::bedrock_converse(),
            ProviderPresetKind::CohereChat => Self::openai_compatible("/chat")
                .with_embedding_path("/embed")
                .with_embedding_body(json!({
                    "model": "{{model}}",
                    "texts": "{{input_texts}}",
                    "input_type": "search_document",
                    "embedding_types": ["float"]
                }))
                .with_embedding_response(EmbeddingResponseManifest {
                    openai_compatible: Some(false),
                    data_path: Some("embeddings.float".to_string()),
                    embedding_path: Some(".".to_string()),
                    ..Default::default()
                }),
        };
        let mut spec = spec.with_base_defaults(base_url);
        spec.capabilities = plugin_preset_capabilities(provider_preset_name(kind))
            .unwrap_or_else(ProviderCapabilities::chat_stream);
        Ok(spec)
    }

    fn openai_compatible(chat_path: impl Into<String>) -> Self {
        Self {
            chat_path: chat_path.into(),
            embedding_path: Some(DEFAULT_EMBEDDINGS_PATH.to_string()),
            headers: Map::new(),
            body: None,
            embedding_body: None,
            stream_path: Some("stream".to_string()),
            response: ResponseManifest {
                openai_compatible: Some(true),
                ..Default::default()
            },
            embedding_response: EmbeddingResponseManifest {
                openai_compatible: Some(true),
                ..Default::default()
            },
            stream: StreamManifest {
                openai_compatible: Some(true),
                ..Default::default()
            },
            adapter: Some(PresetAdapter::OpenaiCompatible),
            capabilities: ProviderCapabilities::openai_compatible_core(),
        }
    }

    fn bedrock_converse() -> Self {
        Self {
            chat_path: "/model/{{model}}/converse".to_string(),
            embedding_path: None,
            headers: Map::from_iter([("Authorization".to_string(), Value::Null)]),
            body: None,
            embedding_body: None,
            stream_path: Some("stream".to_string()),
            response: ResponseManifest {
                openai_compatible: Some(false),
                id_path: None,
                model_path: None,
                content_path: Some("output.message.content.0.text".to_string()),
                finish_reason_path: Some("stopReason".to_string()),
                usage: UsageManifest {
                    prompt_tokens_path: Some("usage.inputTokens".to_string()),
                    completion_tokens_path: Some("usage.outputTokens".to_string()),
                    total_tokens_path: Some("usage.totalTokens".to_string()),
                    cached_tokens_path: None,
                    raw_path: Some("usage".to_string()),
                    output_only_completion_tokens: false,
                    ..Default::default()
                },
                ..Default::default()
            },
            embedding_response: EmbeddingResponseManifest::default(),
            stream: StreamManifest {
                openai_compatible: Some(false),
                event_path: None,
                id_path: None,
                model_path: None,
                role_path: None,
                content_path: Some("output.message.content.0.text".to_string()),
                finish_reason_path: Some("stopReason".to_string()),
                usage: UsageManifest {
                    prompt_tokens_path: Some("usage.inputTokens".to_string()),
                    completion_tokens_path: Some("usage.outputTokens".to_string()),
                    total_tokens_path: Some("usage.totalTokens".to_string()),
                    cached_tokens_path: None,
                    raw_path: Some("usage".to_string()),
                    output_only_completion_tokens: false,
                    ..Default::default()
                },
                ..Default::default()
            },
            adapter: Some(PresetAdapter::BedrockConverse),
            capabilities: plugin_preset_capabilities("bedrock_converse")
                .unwrap_or_else(ProviderCapabilities::chat_stream),
        }
    }

    fn anthropic_messages() -> Self {
        Self {
            chat_path: "/v1/messages".to_string(),
            embedding_path: None,
            headers: Map::from_iter([
                ("x-api-key".to_string(), json!("{{api_key}}")),
                ("anthropic-version".to_string(), json!("2023-06-01")),
                ("Authorization".to_string(), Value::Null),
            ]),
            body: None,
            embedding_body: None,
            stream_path: Some("stream".to_string()),
            response: ResponseManifest {
                openai_compatible: Some(false),
                id_path: Some("id".to_string()),
                model_path: Some("model".to_string()),
                content_path: Some("content.0.text".to_string()),
                finish_reason_path: Some("stop_reason".to_string()),
                usage: UsageManifest {
                    prompt_tokens_path: Some("usage.input_tokens".to_string()),
                    completion_tokens_path: Some("usage.output_tokens".to_string()),
                    total_tokens_path: None,
                    cached_tokens_path: Some("usage.cache_read_input_tokens".to_string()),
                    raw_path: Some("usage".to_string()),
                    output_only_completion_tokens: false,
                    ..Default::default()
                },
                ..Default::default()
            },
            embedding_response: EmbeddingResponseManifest::default(),
            stream: StreamManifest {
                openai_compatible: Some(false),
                event_path: None,
                id_path: Some("message.id".to_string()),
                model_path: Some("message.model".to_string()),
                role_path: Some("message.role".to_string()),
                content_path: Some("delta.text".to_string()),
                finish_reason_path: Some("delta.stop_reason".to_string()),
                usage: UsageManifest {
                    prompt_tokens_path: Some("message.usage.input_tokens".to_string()),
                    completion_tokens_path: Some("usage.output_tokens".to_string()),
                    total_tokens_path: None,
                    cached_tokens_path: Some("message.usage.cache_read_input_tokens".to_string()),
                    raw_path: Some("usage|message.usage".to_string()),
                    output_only_completion_tokens: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            adapter: Some(PresetAdapter::AnthropicMessages),
            capabilities: plugin_preset_capabilities("anthropic_messages")
                .unwrap_or_else(ProviderCapabilities::chat_stream),
        }
    }

    fn with_header(mut self, key: impl Into<String>, value: Value) -> Self {
        self.headers.insert(key.into(), value);
        self
    }

    fn with_embedding_path(mut self, path: impl Into<String>) -> Self {
        self.embedding_path = Some(path.into());
        self
    }

    fn with_embedding_body(mut self, body: Value) -> Self {
        self.embedding_body = Some(body);
        self
    }

    fn with_embedding_response(mut self, response: EmbeddingResponseManifest) -> Self {
        self.embedding_response = response;
        self
    }

    fn without_bearer(self) -> Self {
        self.with_header("Authorization", Value::Null)
    }

    fn with_base_defaults(mut self, base_url: &str) -> Self {
        let base = base_url.trim_end_matches('/');
        if self.chat_path == "/v1beta/openai/chat/completions" && base.ends_with("/v1beta/openai") {
            self.chat_path = DEFAULT_CHAT_PATH.to_string();
            if self.embedding_path.as_deref() == Some("/v1beta/openai/embeddings") {
                self.embedding_path = Some(DEFAULT_EMBEDDINGS_PATH.to_string());
            }
        }
        self
    }
}

pub(crate) fn eval_path_value(value: &Value, expr: &str) -> ProviderResult<Option<Value>> {
    for candidate in split_fallback(expr) {
        match eval_path_candidate(value, candidate)? {
            Some(v) if !is_null_value(v) => return Ok(Some(v.clone())),
            Some(_) | None => {}
        }
    }
    literal_default(expr)
}

fn eval_path_candidate<'a>(value: &'a Value, path: &str) -> ProviderResult<Option<&'a Value>> {
    if path.is_empty() || path == "." || path == "$" {
        return Ok(Some(value));
    }
    let mut cur = value;
    for segment in path.trim_start_matches("$.").split('.') {
        if segment.is_empty() {
            continue;
        }
        cur = match cur {
            Value::Object(map) => match map.get(segment) {
                Some(value) => value,
                None => return Ok(None),
            },
            Value::Array(arr) => {
                let idx = segment.parse::<usize>().map_err(|_| {
                    ProviderError::Config(format!(
                        "invalid array index {segment:?} in path {path:?}"
                    ))
                })?;
                match arr.get(idx) {
                    Some(value) => value,
                    None => return Ok(None),
                }
            }
            _ => return Ok(None),
        };
    }
    Ok(Some(cur))
}

fn split_fallback(expr: &str) -> impl Iterator<Item = &str> {
    expr.split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with("default:") && !s.starts_with("literal:"))
}

fn literal_default(expr: &str) -> ProviderResult<Option<Value>> {
    let default = expr.split('|').map(str::trim).find_map(|part| {
        part.strip_prefix("default:")
            .or_else(|| part.strip_prefix("literal:"))
            .map(str::trim)
    });
    let Some(default) = default else {
        return Ok(None);
    };
    let value = serde_json::from_str(default)
        .map_err(|e| ProviderError::Config(format!("invalid literal default {default:?}: {e}")))?;
    Ok(Some(value))
}

fn is_null_value(value: &Value) -> bool {
    matches!(value, Value::Null)
}

fn extract_u32(value: &Value, path: Option<&str>, field: &str) -> ProviderResult<Option<u32>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let Some(value) = eval_path_value(value, path)? else {
        return Ok(None);
    };
    value_to_u32(&value)
        .ok_or_else(|| {
            ProviderError::Decode(format!(
                "plugin usage {field} at {path:?} is not an unsigned integer"
            ))
        })
        .map(Some)
}

fn extract_f64(value: &Value, path: Option<&str>, field: &str) -> ProviderResult<Option<f64>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let Some(value) = eval_path_value(value, path)? else {
        return Ok(None);
    };
    value_to_f64(&value)
        .ok_or_else(|| {
            ProviderError::Decode(format!("plugin usage {field} at {path:?} is not a number"))
        })
        .map(Some)
}

fn value_to_u32(v: &Value) -> Option<u32> {
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| v.as_str().and_then(|s| s.parse::<u32>().ok()))
}

fn value_to_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
}

pub(super) fn adapt_chat_request(
    req: &ChatRequest,
    adapter: Option<PresetAdapter>,
) -> ProviderResult<Cow<'_, ChatRequest>> {
    match adapter {
        None => Ok(Cow::Borrowed(req)),
        Some(PresetAdapter::OpenaiCompatible) => {
            let mut req = req.clone();
            if req.stream {
                let entry = req
                    .extra
                    .entry("stream_options".to_string())
                    .or_insert_with(|| json!({}));
                match entry {
                    Value::Object(map) => {
                        map.insert("include_usage".to_string(), Value::Bool(true));
                    }
                    slot => *slot = json!({ "include_usage": true }),
                }
            }
            Ok(Cow::Owned(req))
        }
        Some(PresetAdapter::AnthropicMessages) => anthropic_request(req).map(Cow::Owned),
        Some(PresetAdapter::BedrockConverse) => bedrock_converse_request(req).map(Cow::Owned),
    }
}

fn anthropic_request(req: &ChatRequest) -> ProviderResult<ChatRequest> {
    let mut system_parts = Vec::new();
    let mut messages = Vec::new();

    for msg in &req.messages {
        match msg.role {
            Role::System => system_parts.push(msg.content_text().to_string()),
            Role::User | Role::Assistant => {
                messages.push(json!({
                    "role": if msg.role == Role::User { "user" } else { "assistant" },
                    "content": anthropic_content(msg)?,
                }));
            }
            Role::Tool => {
                messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": msg.tool_call_id.clone().unwrap_or_default(),
                        "content": msg.content_text(),
                    }]
                }));
            }
        }
    }

    let mut extra = Map::new();
    extra.insert(
        "max_tokens".to_string(),
        json!(req.max_tokens.unwrap_or(4096)),
    );
    extra.insert("messages".to_string(), Value::Array(messages));
    if !system_parts.is_empty() {
        extra.insert("system".to_string(), json!(system_parts.join("\n")));
    }
    if let Some(t) = req.temperature {
        extra.insert("temperature".to_string(), json!(t));
    }
    if req.stream {
        extra.insert("stream".to_string(), Value::Bool(true));
    }
    if let Some(tools) = &req.tools {
        extra.insert(
            "tools".to_string(),
            Value::Array(
                tools
                    .iter()
                    .map(|td| {
                        json!({
                            "name": td.function.name,
                            "description": td.function.description,
                            "input_schema": td.function.parameters.clone().unwrap_or_else(|| json!({"type":"object"}))
                        })
                    })
                    .collect(),
            ),
        );
    }

    Ok(ChatRequest {
        model: req.model.clone(),
        messages: Vec::new(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: req.stream,
        tools: None,
        tool_choice: None,
        extra,
    })
}

fn anthropic_content(msg: &ChatMessage) -> ProviderResult<Value> {
    if let Some(tool_calls) = &msg.tool_calls {
        let mut blocks = Vec::new();
        let text = msg.content_text();
        if !text.is_empty() {
            blocks.push(json!({ "type": "text", "text": text }));
        }
        for tc in tool_calls {
            let input: Value = serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);
            blocks.push(json!({
                "type": "tool_use",
                "id": tc.id,
                "name": tc.function.name,
                "input": input,
            }));
        }
        return Ok(Value::Array(blocks));
    }

    match &msg.content {
        Some(MessageContent::Parts(parts)) => Ok(Value::Array(
            parts
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text, .. } => json!({ "type": "text", "text": text }),
                    ContentPart::ImageUrl { image_url, .. } => {
                        if image_url.url.starts_with("data:") {
                            let parts: Vec<&str> = image_url.url.splitn(2, ',').collect();
                            let media_type = parts
                                .first()
                                .and_then(|h| h.strip_prefix("data:"))
                                .and_then(|h| h.split(';').next())
                                .unwrap_or("image/png");
                            let data = parts.get(1).copied().unwrap_or_default();
                            json!({
                                "type": "image",
                                "source": { "type": "base64", "media_type": media_type, "data": data }
                            })
                        } else {
                            json!({ "type": "text", "text": format!("[Image: {}]", image_url.url) })
                        }
                    }
                })
                .collect(),
        )),
        _ => Ok(json!(msg.content_text())),
    }
}

fn bedrock_converse_request(req: &ChatRequest) -> ProviderResult<ChatRequest> {
    let mut system = Vec::new();
    let mut messages = Vec::new();

    for msg in &req.messages {
        match msg.role {
            Role::System => system.push(json!({ "text": msg.content_text() })),
            Role::User | Role::Assistant => messages.push(json!({
                "role": if msg.role == Role::User { "user" } else { "assistant" },
                "content": [{ "text": msg.content_text() }],
            })),
            Role::Tool => messages.push(json!({
                "role": "user",
                "content": [{ "text": msg.content_text() }],
            })),
        }
    }

    let mut extra = Map::new();
    extra.insert("messages".to_string(), Value::Array(messages));
    if !system.is_empty() {
        extra.insert("system".to_string(), Value::Array(system));
    }
    extra.insert(
        "inferenceConfig".to_string(),
        json!({
            "maxTokens": req.max_tokens,
            "temperature": req.temperature,
            "topP": req.top_p,
        }),
    );

    Ok(ChatRequest {
        model: req.model.clone(),
        messages: Vec::new(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: req.stream,
        tools: None,
        tool_choice: None,
        extra,
    })
}
