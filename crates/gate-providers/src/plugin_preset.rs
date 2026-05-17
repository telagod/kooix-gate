//! Built-in provider presets for the runtime HTTP plugin.

use crate::error::ProviderResult;
use crate::types::*;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::borrow::Cow;

const DEFAULT_CHAT_PATH: &str = "/chat/completions";

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProviderPresetKind {
    Openai,
    OpenaiCompatible,
    Deepseek,
    Mistral,
    Gemini,
    AzureOpenai,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PresetAdapter {
    OpenaiCompatible,
    AnthropicMessages,
    BedrockConverse,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub(super) struct PresetManifest {
    #[serde(alias = "provider")]
    pub(super) kind: Option<ProviderPresetKind>,
    pub(super) api_version: Option<String>,
    #[serde(skip)]
    pub(super) adapter: Option<PresetAdapter>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub(super) struct ResponseManifest {
    pub(super) openai_compatible: Option<bool>,
    pub(super) id_path: Option<String>,
    pub(super) model_path: Option<String>,
    pub(super) content_path: Option<String>,
    pub(super) finish_reason_path: Option<String>,
    pub(super) usage: UsageManifest,
}

impl ResponseManifest {
    pub(super) fn apply_defaults(&mut self, defaults: Self) {
        self.openai_compatible = self.openai_compatible.or(defaults.openai_compatible);
        self.id_path = self.id_path.take().or(defaults.id_path);
        self.model_path = self.model_path.take().or(defaults.model_path);
        self.content_path = self.content_path.take().or(defaults.content_path);
        self.finish_reason_path = self
            .finish_reason_path
            .take()
            .or(defaults.finish_reason_path);
        self.usage.apply_defaults(defaults.usage);
    }

    pub(super) fn is_openai_compatible(&self) -> bool {
        self.openai_compatible.unwrap_or(true)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub(super) struct StreamManifest {
    pub(super) openai_compatible: Option<bool>,
    pub(super) event_path: Option<String>,
    pub(super) done: Vec<String>,
    pub(super) id_path: Option<String>,
    pub(super) model_path: Option<String>,
    pub(super) role_path: Option<String>,
    pub(super) content_path: Option<String>,
    pub(super) finish_reason_path: Option<String>,
    pub(super) usage: UsageManifest,
}

impl StreamManifest {
    pub(super) fn apply_defaults(&mut self, defaults: Self) {
        self.openai_compatible = self.openai_compatible.or(defaults.openai_compatible);
        self.event_path = self.event_path.take().or(defaults.event_path);
        if self.done.is_empty() {
            self.done = defaults.done;
        }
        self.id_path = self.id_path.take().or(defaults.id_path);
        self.model_path = self.model_path.take().or(defaults.model_path);
        self.role_path = self.role_path.take().or(defaults.role_path);
        self.content_path = self.content_path.take().or(defaults.content_path);
        self.finish_reason_path = self
            .finish_reason_path
            .take()
            .or(defaults.finish_reason_path);
        self.usage.apply_defaults(defaults.usage);
    }

    pub(super) fn is_openai_compatible(&self) -> bool {
        self.openai_compatible.unwrap_or(true)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub(super) struct UsageManifest {
    pub(super) prompt_tokens_path: Option<String>,
    pub(super) completion_tokens_path: Option<String>,
    pub(super) total_tokens_path: Option<String>,
    pub(super) cached_tokens_path: Option<String>,
    pub(super) output_only_completion_tokens: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ExtractedUsage {
    pub(super) usage: Usage,
    pub(super) completion_present: bool,
    pub(super) total_present: bool,
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
        self.output_only_completion_tokens |= defaults.output_only_completion_tokens;
    }

    pub(super) fn extract(&self, value: &Value) -> Usage {
        self.extract_with_presence(value).usage
    }

    fn extract_with_presence(&self, value: &Value) -> ExtractedUsage {
        let prompt = self
            .prompt_tokens_path
            .as_deref()
            .and_then(|p| get_path(value, p))
            .and_then(value_to_u32)
            .unwrap_or_default();
        let completion_raw = self
            .completion_tokens_path
            .as_deref()
            .and_then(|p| get_path(value, p))
            .and_then(value_to_u32);
        let completion = completion_raw.unwrap_or_default();
        let total_raw = self
            .total_tokens_path
            .as_deref()
            .and_then(|p| get_path(value, p))
            .and_then(value_to_u32);
        let total = total_raw.unwrap_or_else(|| prompt + completion);
        let cached = self
            .cached_tokens_path
            .as_deref()
            .and_then(|p| get_path(value, p))
            .and_then(value_to_u32)
            .unwrap_or_default();

        ExtractedUsage {
            usage: Usage {
                prompt_tokens: prompt,
                completion_tokens: completion,
                total_tokens: total,
                cached_tokens: cached,
            },
            completion_present: completion_raw.is_some(),
            total_present: total_raw.is_some(),
        }
    }

    pub(super) fn extract_optional(&self, value: &Value) -> Option<ExtractedUsage> {
        let usage = self.extract_with_presence(value);
        (usage.usage.prompt_tokens > 0
            || usage.usage.completion_tokens > 0
            || usage.usage.total_tokens > 0)
            .then_some(usage)
    }
}

#[derive(Debug)]
pub(super) struct ProviderPresetSpec {
    pub(super) chat_path: String,
    pub(super) headers: Map<String, Value>,
    pub(super) body: Option<Value>,
    pub(super) stream_path: Option<String>,
    pub(super) response: ResponseManifest,
    pub(super) stream: StreamManifest,
    pub(super) adapter: Option<PresetAdapter>,
}

impl ProviderPresetSpec {
    pub(super) fn for_kind(
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
            | ProviderPresetKind::Ollama => Self::openai_compatible(DEFAULT_CHAT_PATH),
            ProviderPresetKind::Gemini => {
                Self::openai_compatible("/v1beta/openai/chat/completions")
            }
            ProviderPresetKind::AzureOpenai => Self::openai_compatible(format!(
                "/openai/deployments/{{{{model}}}}/chat/completions?api-version={}",
                api_version.unwrap_or("2024-08-01-preview")
            ))
            .with_header("api-key", json!("{{api_key}}"))
            .without_bearer(),
            ProviderPresetKind::AnthropicMessages => Self::anthropic_messages(),
            ProviderPresetKind::BedrockConverse => Self::bedrock_converse(),
            ProviderPresetKind::CohereChat => Self::openai_compatible("/chat"),
        };
        Ok(spec.with_base_defaults(base_url))
    }

    fn openai_compatible(chat_path: impl Into<String>) -> Self {
        Self {
            chat_path: chat_path.into(),
            headers: Map::new(),
            body: None,
            stream_path: Some("stream".to_string()),
            response: ResponseManifest {
                openai_compatible: Some(true),
                ..Default::default()
            },
            stream: StreamManifest {
                openai_compatible: Some(true),
                ..Default::default()
            },
            adapter: Some(PresetAdapter::OpenaiCompatible),
        }
    }

    fn bedrock_converse() -> Self {
        Self {
            chat_path: "/model/{{model}}/converse".to_string(),
            headers: Map::from_iter([
                ("X-Amz-Access-Key".to_string(), json!("{{api_key}}")),
                ("X-Amz-Secret-Key".to_string(), json!("{{aws_secret_key}}")),
                ("Authorization".to_string(), Value::Null),
            ]),
            body: None,
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
                    output_only_completion_tokens: false,
                },
            },
            stream: StreamManifest {
                openai_compatible: Some(false),
                event_path: None,
                done: Vec::new(),
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
                    output_only_completion_tokens: false,
                },
            },
            adapter: Some(PresetAdapter::BedrockConverse),
        }
    }

    fn anthropic_messages() -> Self {
        Self {
            chat_path: "/v1/messages".to_string(),
            headers: Map::from_iter([
                ("x-api-key".to_string(), json!("{{api_key}}")),
                ("anthropic-version".to_string(), json!("2023-06-01")),
                ("Authorization".to_string(), Value::Null),
            ]),
            body: None,
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
                    output_only_completion_tokens: false,
                },
            },
            stream: StreamManifest {
                openai_compatible: Some(false),
                event_path: None,
                done: Vec::new(),
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
                    output_only_completion_tokens: true,
                },
            },
            adapter: Some(PresetAdapter::AnthropicMessages),
        }
    }

    fn with_header(mut self, key: impl Into<String>, value: Value) -> Self {
        self.headers.insert(key.into(), value);
        self
    }

    fn without_bearer(self) -> Self {
        self.with_header("Authorization", Value::Null)
    }

    fn with_base_defaults(mut self, base_url: &str) -> Self {
        let base = base_url.trim_end_matches('/');
        if self.chat_path == "/v1beta/openai/chat/completions" && base.ends_with("/v1beta/openai") {
            self.chat_path = DEFAULT_CHAT_PATH.to_string();
        }
        self
    }
}

fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() || path == "." || path == "$" {
        return Some(value);
    }
    let mut cur = value;
    for segment in path.trim_start_matches("$.").split('.') {
        if segment.is_empty() {
            continue;
        }
        cur = match cur {
            Value::Object(map) => map.get(segment)?,
            Value::Array(arr) => arr.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn value_to_u32(v: &Value) -> Option<u32> {
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| v.as_str().and_then(|s| s.parse::<u32>().ok()))
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
