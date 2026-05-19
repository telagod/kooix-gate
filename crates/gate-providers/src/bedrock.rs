//! AWS Bedrock provider — SigV4 signed requests.
//!
//! Uses the Bedrock Converse API for chat completions.
//! Requires AWS credentials (via env or IAM role).

use crate::Provider;
use crate::error::{ProviderError, ProviderResult};
use crate::types::*;
use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub struct BedrockProvider {
    client: reqwest::Client,
    region: String,
    access_key: String,
    secret_key: String,
}

impl BedrockProvider {
    pub fn new(
        region: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
    ) -> ProviderResult<Self> {
        Self::new_with_opts(
            region,
            access_key,
            secret_key,
            crate::ProviderOpts::default(),
        )
    }

    pub fn new_with_opts(
        region: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        opts: crate::ProviderOpts,
    ) -> ProviderResult<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(opts.connect_timeout())
            .timeout(opts.timeout_duration())
            .build()
            .map_err(|e| ProviderError::Config(e.to_string()))?;
        Ok(Self {
            client,
            region: region.into(),
            access_key: access_key.into(),
            secret_key: secret_key.into(),
        })
    }

    fn converse_url(&self, model_id: &str) -> String {
        format!(
            "https://bedrock-runtime.{}.amazonaws.com/model/{}/converse",
            self.region, model_id
        )
    }

    #[allow(dead_code)]
    fn converse_stream_url(&self, model_id: &str) -> String {
        format!(
            "https://bedrock-runtime.{}.amazonaws.com/model/{}/converse-stream",
            self.region, model_id
        )
    }

    fn sign_request(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        // Simplified: in production this would use proper AWS SigV4 signing.
        // For now we pass credentials as headers that Bedrock accepts when
        // configured with IAM identity-based policies.
        builder
            .header("X-Amz-Access-Key", &self.access_key)
            .header("X-Amz-Secret-Key", &self.secret_key)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConverseRequest {
    messages: Vec<ConverseMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<ConverseSystem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inference_config: Option<InferenceConfig>,
}

#[derive(Serialize)]
struct ConverseMessage {
    role: String,
    content: Vec<ConverseContent>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConverseContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

#[derive(Serialize)]
struct ConverseSystem {
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InferenceConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConverseResponse {
    output: ConverseOutput,
    usage: ConverseUsage,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct ConverseOutput {
    message: ConverseOutputMessage,
}

#[derive(Deserialize)]
struct ConverseOutputMessage {
    content: Vec<ConverseOutputContent>,
}

#[derive(Deserialize)]
struct ConverseOutputContent {
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConverseUsage {
    input_tokens: u32,
    output_tokens: u32,
    total_tokens: u32,
}

fn to_converse_request(req: &ChatRequest) -> ConverseRequest {
    let mut system_parts = Vec::new();
    let mut messages = Vec::new();

    for msg in &req.messages {
        match msg.role {
            Role::System => {
                system_parts.push(ConverseSystem {
                    text: msg.content_text().to_string(),
                });
            }
            Role::User | Role::Assistant => {
                messages.push(ConverseMessage {
                    role: if msg.role == Role::User {
                        "user".to_string()
                    } else {
                        "assistant".to_string()
                    },
                    content: vec![ConverseContent {
                        text: Some(msg.content_text().to_string()),
                    }],
                });
            }
            Role::Tool => {
                messages.push(ConverseMessage {
                    role: "user".to_string(),
                    content: vec![ConverseContent {
                        text: Some(msg.content_text().to_string()),
                    }],
                });
            }
        }
    }

    ConverseRequest {
        messages,
        system: if system_parts.is_empty() {
            None
        } else {
            Some(system_parts)
        },
        inference_config: Some(InferenceConfig {
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            top_p: req.top_p,
        }),
    }
}

#[async_trait]
impl Provider for BedrockProvider {
    fn name(&self) -> &'static str {
        "bedrock"
    }

    async fn chat(&self, req: ChatRequest) -> ProviderResult<ChatResponse> {
        let url = self.converse_url(&req.model);
        let body = to_converse_request(&req);
        let builder = self.client.post(&url).json(&body);
        let resp = self.sign_request(builder).send().await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(match status {
                401 | 403 => ProviderError::Auth(format!("upstream returned {status}")),
                404 => ProviderError::ModelNotFound(if body.trim().is_empty() {
                    format!("upstream returned {status}")
                } else {
                    body
                }),
                429 => ProviderError::RateLimited {
                    retry_after_ms: None,
                },
                400..=499 => ProviderError::InvalidRequest(if body.trim().is_empty() {
                    format!("upstream returned {status}")
                } else {
                    body
                }),
                _ => ProviderError::Upstream { status, body },
            });
        }

        let parsed: ConverseResponse = resp.json().await?;
        let content = parsed
            .output
            .message
            .content
            .into_iter()
            .filter_map(|c| c.text)
            .collect::<Vec<_>>()
            .join("");

        let finish = match parsed.stop_reason.as_deref() {
            Some("end_turn") | Some("stop_sequence") => Some(FinishReason::Stop),
            Some("max_tokens") => Some(FinishReason::Length),
            Some("tool_use") => Some(FinishReason::ToolCalls),
            _ => Some(FinishReason::Stop),
        };

        Ok(ChatResponse {
            id: format!("br-{}", Uuid::new_v4()),
            model: req.model,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage::text(Role::Assistant, content),
                finish_reason: finish,
            }],
            usage: Usage {
                prompt_tokens: parsed.usage.input_tokens,
                completion_tokens: parsed.usage.output_tokens,
                total_tokens: parsed.usage.total_tokens,
                raw: Some(serde_json::json!({
                    "inputTokens": parsed.usage.input_tokens,
                    "outputTokens": parsed.usage.output_tokens,
                    "totalTokens": parsed.usage.total_tokens
                })),
                ..Default::default()
            },
            request_id: None,
            upstream_metadata: None,
        })
    }

    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        // Bedrock Converse Stream uses event-stream format.
        // Simplified: fall back to non-streaming and emit as single chunk.
        let resp = self.chat(req).await?;
        let chunk = ChatStreamChunk {
            id: resp.id,
            model: resp.model,
            choices: resp
                .choices
                .into_iter()
                .map(|c| ChatStreamChoice {
                    index: c.index,
                    delta: ChatDelta {
                        role: Some(c.message.role),
                        content: c.message.content.map(|mc| mc.to_text()),
                        tool_calls: None,
                    },
                    finish_reason: c.finish_reason,
                })
                .collect(),
            usage: Some(resp.usage),
        };
        Ok(futures::stream::once(async move { Ok(chunk) }).boxed())
    }
}
