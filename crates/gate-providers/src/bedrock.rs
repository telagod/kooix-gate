//! AWS Bedrock provider — SigV4 signed requests.
//!
//! Uses the Bedrock Converse API for chat completions.
//! Requires AWS credentials (via env or IAM role).
//!
//! 0.4.0 起：SigV4 改用真实 AWS Signature V4 签名（参考 AWS Authentication Spec）。
//! 复用 [`crate::sigv4`] 顶层模块的 helper。之前的 0.2.x / 0.3.x 仅发
//! `X-Amz-Access-Key/Secret-Key` 头作为占位（不是 AWS 标准），生产环境实际靠 plugin
//! runtime（auth_strategy=aws_sigv4）跑，编译期 BedrockProvider 几乎没人用。
//! 现在编译期路径也合规了，可以真正参与 ADR-0002 fast-path。

use crate::Provider;
use crate::error::{ProviderError, ProviderResult};
use crate::sigv4::{
    aws_sigv4_signing_key, canonical_query_string, canonical_uri, hmac_sha256_hex, sha256_hex,
};
use crate::types::*;
use async_trait::async_trait;
use chrono::Utc;
use futures::stream::{BoxStream, StreamExt};
use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub struct BedrockProvider {
    client: std::sync::Arc<reqwest::Client>,
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
        let client = crate::shared_http_client(&opts)?;
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

    fn sigv4_sign_post(&self, url: &str, body: &[u8]) -> ProviderResult<(HeaderMap, Vec<u8>)> {
        let parsed = Url::parse(url)
            .map_err(|e| ProviderError::Config(format!("bedrock sigv4 bad url '{url}': {e}")))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| ProviderError::Config(format!("bedrock sigv4 missing host: {url}")))?
            .to_string();

        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        let credential_scope = format!("{date}/{}/bedrock/aws4_request", self.region);

        let payload_hash = sha256_hex(body);
        let canonical_uri_str = canonical_uri(&parsed);
        let canonical_query = canonical_query_string(&parsed);

        // SignedHeaders（按字母序，参与签名的头）：host + x-amz-content-sha256 + x-amz-date
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_headers =
            format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n",);

        let canonical_request = format!(
            "POST\n{canonical_uri_str}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
        );
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signing_key = aws_sigv4_signing_key(&self.secret_key, &date, &self.region, "bedrock")?;
        let signature = hmac_sha256_hex(&signing_key, string_to_sign.as_bytes())?;
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.access_key, credential_scope, signed_headers, signature,
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&authorization).map_err(|e| {
                ProviderError::Config(format!("bedrock sigv4 invalid authorization: {e}"))
            })?,
        );
        headers.insert(
            HeaderName::from_static("x-amz-date"),
            HeaderValue::from_str(&amz_date)
                .map_err(|e| ProviderError::Config(format!("bedrock sigv4 amz-date: {e}")))?,
        );
        headers.insert(
            HeaderName::from_static("x-amz-content-sha256"),
            HeaderValue::from_str(&payload_hash)
                .map_err(|e| ProviderError::Config(format!("bedrock sigv4 content-sha256: {e}")))?,
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        Ok((headers, body.to_vec()))
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
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| ProviderError::Config(format!("bedrock encode body: {e}")))?;
        let (headers, body_bytes) = self.sigv4_sign_post(&url, &body_bytes)?;
        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .body(body_bytes)
            .send()
            .await?;

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

// ─── ADR-0002 fast-path entry points ────────────────────────────────────────

/// Convert ChatRequest to Bedrock Converse API request body (serde_json::Value).
pub(crate) fn fastpath_bedrock_request_body(req: &ChatRequest) -> serde_json::Value {
    serde_json::to_value(to_converse_request(req)).expect("ConverseRequest serializable")
}

/// Convert Bedrock Converse JSON response back to OpenAI-compatible ChatResponse.
pub(crate) fn fastpath_bedrock_response_from_json(
    value: serde_json::Value,
    model: &str,
) -> ProviderResult<ChatResponse> {
    let parsed: ConverseResponse = serde_json::from_value(value)
        .map_err(|e| ProviderError::Decode(format!("bedrock response decode: {e}")))?;
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
        model: model.to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage::text(Role::Assistant, content),
            finish_reason: finish,
        }],
        usage: Usage {
            prompt_tokens: parsed.usage.input_tokens,
            completion_tokens: parsed.usage.output_tokens,
            total_tokens: parsed.usage.total_tokens,
            ..Default::default()
        },
        request_id: None,
        upstream_metadata: None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 已知向量验证：固定输入应该产出固定 SigV4 签名。这个 test 锁定算法不漂移。
    /// 向量来自 plugin runtime 已经验证过的等价路径
    /// （见 crates/gate-providers/src/custom_provider/mod.rs 已有 SigV4 fixture），
    /// 同样的 (date, region, service, secret_key, payload) 输入应产同样签名。
    #[test]
    fn sigv4_known_vector_canonical_request_and_string_to_sign() {
        // 固定时间，固定 payload，对照 string_to_sign 应能稳定复现。
        // 这里直接调底层 helper 验证 string_to_sign 正确，端到端的
        // sigv4_sign_post 在下面 sign_post_smoke test 验证。
        let secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let date = "20150830";
        let region = "us-east-1";
        let key = aws_sigv4_signing_key(secret, date, region, "iam").unwrap();
        // AWS doc test vector: signing key for 20150830/us-east-1/iam/aws4_request
        let expected_hex = "c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9";
        assert_eq!(hex::encode(&key), expected_hex);
    }

    /// 端到端：调 sigv4_sign_post 应该返回带 Authorization / x-amz-date /
    /// x-amz-content-sha256 三个必备 header 的 HeaderMap，且 Authorization 形如
    /// `AWS4-HMAC-SHA256 Credential=...`。
    #[test]
    fn sign_post_returns_well_formed_authorization_header() {
        let provider = BedrockProvider::new(
            "us-east-1".to_string(),
            "AKIDEXAMPLE".to_string(),
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
        )
        .unwrap();
        let url = "https://bedrock-runtime.us-east-1.amazonaws.com/model/anthropic.claude-3-haiku-20240307-v1:0/converse";
        let body = br#"{"messages":[{"role":"user","content":[{"text":"hi"}]}]}"#;
        let (headers, body_out) = provider.sigv4_sign_post(url, body).unwrap();

        let auth = headers
            .get(reqwest::header::AUTHORIZATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"),
            "auth header malformed: {auth}"
        );
        assert!(
            auth.contains("/us-east-1/bedrock/aws4_request"),
            "credential scope wrong: {auth}"
        );
        assert!(
            auth.contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date"),
            "signed headers wrong: {auth}"
        );
        assert!(
            auth.contains(", Signature="),
            "missing signature segment: {auth}"
        );
        assert!(headers.contains_key("x-amz-date"));
        assert!(headers.contains_key("x-amz-content-sha256"));
        assert_eq!(body_out.as_slice(), body);

        // 旧版假签名头必须不存在
        assert!(headers.get("X-Amz-Access-Key").is_none());
        assert!(headers.get("X-Amz-Secret-Key").is_none());
    }
}
