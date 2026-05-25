//! ADR-0002 fast-path runtime — OpenAI / Anthropic / Azure / Bedrock 4 个高频 provider
//! 走静态分发，跳过 manifest 解释器，性能回归编译期水准（× 0.74-1.00）。
//!
//! 详见 docs/architecture/decisions/ADR-0002-fastpath-runtime.md。

use super::*;
use crate::error::{ProviderError, ProviderResult};
use crate::plugin_preset::ProviderPresetKind;
use crate::types::*;
use futures::stream::BoxStream;

// ─── ADR-0002 fast-path runtime ─────────────────────────────────────────────
//
// 4 个 fast-path provider 在 `apply_preset` 阶段被打上 `security.builtin_fastpath`，
// trait impl 顶部分发到下面这些函数：跳过 manifest 模板 / placeholder render /
// auth header dispatch，直接复用编译期 OpenAI 路径。
//
// 0.3.x 仅实现 OpenAI 一条；剩下 3 条（Anthropic Messages / Azure OpenAI /
// Bedrock SigV4）在 0.4.0 接入。其余 provider 进 trait impl 后走 manifest 解释器
// 老路。
//
// **Panic 兜底**：fast-path 走的是手写代码路径，理论上不会 panic；但万一
// （比如 OpenAI 改了响应格式触发 serde panic），`try_fastpath_*` 用
// `FutureExt::catch_unwind` 兜底，panic 时记录 error 并降级到 manifest runtime
// 老路，进程不挂。
impl CustomHttpProvider {
    #[inline]
    pub(super) fn fastpath_kind(&self) -> Option<ProviderPresetKind> {
        if self.manifest.security.builtin_fastpath {
            self.manifest.preset.kind
        } else {
            None
        }
    }

    /// 用 catch_unwind 包裹 fast-path 调用：panic → 记录 error + 返回 None
    /// 让外层降级；正常路径返回 Some(result)。
    ///
    /// 注意：catch_unwind 要求 `UnwindSafe`，async future 自身用
    /// `AssertUnwindSafe` 包；正常返回的 Result 已经是 Send+'static。
    pub(super) async fn run_fastpath<T>(
        &self,
        kind: ProviderPresetKind,
        op: &'static str,
        fut: impl std::future::Future<Output = ProviderResult<T>>,
    ) -> Option<ProviderResult<T>> {
        use futures::FutureExt;
        use std::panic::AssertUnwindSafe;
        match AssertUnwindSafe(fut).catch_unwind().await {
            Ok(result) => Some(result),
            Err(panic_payload) => {
                let msg = panic_message(&panic_payload);
                tracing::error!(
                    target: "kooix::providers::fastpath",
                    preset = ?kind,
                    op = op,
                    panic = %msg,
                    "fast-path panicked; falling back to manifest runtime",
                );
                None
            }
        }
    }

    /// OpenAI fast-path：直接 POST `{base_url}/chat/completions`，Bearer 鉴权，
    /// JSON body 就是 `ChatRequest`。等价于 `OpenAiProvider::chat`，但保留
    /// sandbox dns + peer 校验。
    pub(super) async fn fastpath_openai_chat(
        &self,
        mut req: ChatRequest,
    ) -> ProviderResult<ChatResponse> {
        req.stream = false;
        let url = format!("{}/chat/completions", self.base_url);
        let api_key = self.secret_for_slot("primary");
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        Ok(resp.json().await?)
    }

    pub(super) async fn fastpath_openai_chat_stream(
        &self,
        mut req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        req.stream = true;
        // 与 OpenAiProvider 一致：force include_usage 给计费用
        let entry = req
            .extra
            .entry("stream_options".to_string())
            .or_insert_with(|| json!({}));
        match entry {
            Value::Object(map) => {
                map.insert("include_usage".to_string(), Value::Bool(true));
            }
            slot => {
                *slot = json!({ "include_usage": true });
            }
        }
        let url = format!("{}/chat/completions", self.base_url);
        let api_key = self.secret_for_slot("primary");
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        check_status(&resp)?;
        Ok(sse_to_chunks(resp.bytes_stream()).boxed())
    }

    pub(super) async fn fastpath_openai_embed(
        &self,
        req: EmbeddingRequest,
    ) -> ProviderResult<EmbeddingResponse> {
        let url = format!("{}/embeddings", self.base_url);
        let api_key = self.secret_for_slot("primary");
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        Ok(resp.json().await?)
    }

    /// Anthropic Messages fast-path：POST `{base_url}/v1/messages`，
    /// `x-api-key` + `anthropic-version` 头，body 用 Anthropic 原生格式（system /
    /// content blocks / tool_use / tool_result），响应映射回 OpenAI ChatResponse。
    /// 复用 `crate::anthropic` 模块的 helper，**不重复实现协议**。
    pub(super) async fn fastpath_anthropic_chat(
        &self,
        req: ChatRequest,
    ) -> ProviderResult<ChatResponse> {
        use crate::anthropic::{
            FASTPATH_ANTHROPIC_VERSION, fastpath_anthropic_check_status,
            fastpath_anthropic_request_body, fastpath_anthropic_response_from_json,
        };
        let url = format!("{}/v1/messages", self.base_url);
        let api_key = self.secret_for_slot("primary");
        let body = fastpath_anthropic_request_body(&req);
        let resp = self
            .client
            .post(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", FASTPATH_ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        fastpath_anthropic_check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        let value: Value = resp.json().await?;
        fastpath_anthropic_response_from_json(value)
    }

    pub(super) async fn fastpath_anthropic_chat_stream(
        &self,
        req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        use crate::anthropic::{
            FASTPATH_ANTHROPIC_VERSION, fastpath_anthropic_check_status,
            fastpath_anthropic_request_body, fastpath_anthropic_sse_stream,
        };
        let url = format!("{}/v1/messages", self.base_url);
        let api_key = self.secret_for_slot("primary");
        let mut body = fastpath_anthropic_request_body(&req);
        // Anthropic SSE 要 stream:true 字段
        if let Value::Object(map) = &mut body {
            map.insert("stream".to_string(), Value::Bool(true));
        }
        let resp = self
            .client
            .post(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", FASTPATH_ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        fastpath_anthropic_check_status(&resp)?;
        Ok(fastpath_anthropic_sse_stream(resp.bytes_stream()).boxed())
    }

    /// Azure OpenAI fast-path：deployment-based URL + `api-key` 头。
    /// 请求/响应 body 与 OpenAI 一致，所以复用 OpenAI 的 check_status / sse_to_chunks。
    pub(super) fn azure_chat_url(&self, model: &str) -> String {
        let api_version = self
            .manifest
            .preset
            .api_version
            .as_deref()
            .unwrap_or("2024-08-01-preview");
        format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            self.base_url, model, api_version
        )
    }

    pub(super) fn azure_embeddings_url(&self, model: &str) -> String {
        let api_version = self
            .manifest
            .preset
            .api_version
            .as_deref()
            .unwrap_or("2024-08-01-preview");
        format!(
            "{}/openai/deployments/{}/embeddings?api-version={}",
            self.base_url, model, api_version
        )
    }

    pub(super) async fn fastpath_azure_chat(
        &self,
        mut req: ChatRequest,
    ) -> ProviderResult<ChatResponse> {
        req.stream = false;
        let url = self.azure_chat_url(&req.model);
        let api_key = self.secret_for_slot("primary");
        let resp = self
            .client
            .post(&url)
            .header("api-key", api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        Ok(resp.json().await?)
    }

    pub(super) async fn fastpath_azure_chat_stream(
        &self,
        mut req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        req.stream = true;
        let entry = req
            .extra
            .entry("stream_options".to_string())
            .or_insert_with(|| json!({}));
        match entry {
            Value::Object(map) => {
                map.insert("include_usage".to_string(), Value::Bool(true));
            }
            slot => {
                *slot = json!({ "include_usage": true });
            }
        }
        let url = self.azure_chat_url(&req.model);
        let api_key = self.secret_for_slot("primary");
        let resp = self
            .client
            .post(&url)
            .header("api-key", api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        check_status(&resp)?;
        Ok(sse_to_chunks(resp.bytes_stream()).boxed())
    }

    pub(super) async fn fastpath_azure_embed(
        &self,
        req: EmbeddingRequest,
    ) -> ProviderResult<EmbeddingResponse> {
        let url = self.azure_embeddings_url(&req.model);
        let api_key = self.secret_for_slot("primary");
        let resp = self
            .client
            .post(&url)
            .header("api-key", api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        Ok(resp.json().await?)
    }

    /// Bedrock Converse fast-path：AWS SigV4 签名 + 原生 Converse body，
    /// 复用 [`crate::sigv4`] 的 helper（与 manifest runtime 路径字节级等价），
    /// 复用 [`crate::bedrock`] 的 request/response 转换 helper（零协议重复）。
    ///
    /// region 从 base_url host 推（默认 us-east-1 兜底），
    /// AWS 凭证从 secret slot `aws_access_key` / `aws_secret_key` 拿（标准 plugin slot）。
    pub(super) async fn fastpath_bedrock_chat(
        &self,
        req: ChatRequest,
    ) -> ProviderResult<ChatResponse> {
        use crate::bedrock::{fastpath_bedrock_request_body, fastpath_bedrock_response_from_json};
        use crate::sigv4::{
            aws_sigv4_signing_key, canonical_query_string, canonical_uri, hmac_sha256_hex,
            infer_aws_region_from_host, sha256_hex,
        };

        let url = format!("{}/model/{}/converse", self.base_url, req.model);
        let parsed = Url::parse(&url)
            .map_err(|e| ProviderError::Config(format!("bedrock fastpath bad url '{url}': {e}")))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| ProviderError::Config("bedrock fastpath missing host".to_string()))?
            .to_string();
        let region = infer_aws_region_from_host(&host)
            .or_else(|| std::env::var("AWS_REGION").ok())
            .unwrap_or_else(|| "us-east-1".to_string());
        let access_key = self.secret_for_slot("aws_access_key");
        let secret_key = self.secret_for_slot("aws_secret_key");
        if access_key.is_empty() || secret_key.is_empty() {
            return Err(ProviderError::Config(
                "bedrock fastpath requires aws_access_key + aws_secret_key secret slots"
                    .to_string(),
            ));
        }

        let body_value = fastpath_bedrock_request_body(&req);
        let body_bytes = serde_json::to_vec(&body_value)
            .map_err(|e| ProviderError::Config(format!("bedrock encode body: {e}")))?;

        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        let credential_scope = format!("{date}/{region}/bedrock/aws4_request");
        let payload_hash = sha256_hex(&body_bytes);
        let canonical_uri_str = canonical_uri(&parsed);
        let canonical_query = canonical_query_string(&parsed);
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
        let signing_key = aws_sigv4_signing_key(&secret_key, &date, &region, "bedrock")?;
        let signature = hmac_sha256_hex(&signing_key, string_to_sign.as_bytes())?;
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={access_key}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
        );

        let resp = self
            .client
            .post(&url)
            .header("authorization", authorization)
            .header("x-amz-date", amz_date)
            .header("x-amz-content-sha256", payload_hash)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body_bytes)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(match status {
                401 | 403 => ProviderError::Auth(format!("bedrock returned {status}: {body}")),
                404 => ProviderError::ModelNotFound(format!("bedrock returned {status}: {body}")),
                429 => ProviderError::RateLimited {
                    retry_after_ms: None,
                },
                _ => ProviderError::upstream(status, body),
            });
        }
        let value: Value = resp.json().await?;
        fastpath_bedrock_response_from_json(value, &req.model)
    }
}
