//! Runtime-configurable HTTP provider plugin.
//!
//! A `provider_type` of `plugin` / `custom` / `http` uses channel `model_mapping`
//! as the plugin manifest. The manifest can reshape requests, map strange JSON
//! responses, and normalize arbitrary SSE frames back into OpenAI-compatible chunks.

mod fastpath;
mod helpers;
mod replay;
mod sandbox;
mod secrets;
mod sigv4;

use helpers::{
    code_matches, embedding_request_context, enforce_response_length_hint, enforce_size,
    insert_header, insert_named_header, map_finish_reason, map_role, message_contains_any,
    panic_message, parse_embedding_vector, render_template, render_template_str, render_value,
    request_context, retry_after_ms, set_path, slash_path, status_in, value_to_string,
    value_to_u16, value_to_u32,
};
use secrets::{
    env_key_for_secret_slot, env_secret_slots, normalize_secret_slot, normalize_secret_slots,
};
use sigv4::{
    aws_sigv4_signing_key, canonical_query_string, canonical_uri, hmac_sha256_hex,
    infer_aws_region_from_host, sha256_hex,
};

use crate::error::{
    NormalizedProviderErrorKind, ProviderError, ProviderErrorMetadata, ProviderResult,
};
use crate::openai::{check_status, sse_to_chunks};
use crate::plugin_manifest::{
    AuthStrategy, DEFAULT_CHAT_PATH, DEFAULT_EMBEDDINGS_PATH, PluginManifest, ProbeManifest,
    RequestMethod, SignatureEncoding,
};
use crate::plugin_preset::{ProviderPresetKind, adapt_chat_request, eval_path_value};
use crate::types::*;
use crate::{EmbeddingProvider, Provider};
use async_trait::async_trait;
use base64::Engine as _;
use futures::stream::{BoxStream, StreamExt};
use hmac::Mac;
pub use replay::replay_plugin_sse;
use replay::{StreamMapper, merge_reasoning_content, normalize_plugin_sse};
use reqwest::Method;
use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use sandbox::{EndpointKind, PluginHttpSandbox, SandboxDnsResolver};
use serde::Deserialize;
use serde_json::{Value, json};
use std::borrow::Cow;
use std::collections::HashMap;
#[cfg(test)]
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

pub(super) use crate::sigv4::HmacSha256;

#[derive(Debug)]
struct AwsSigv4Signature {
    authorization: String,
    #[cfg(test)]
    canonical_request: String,
    #[cfg(test)]
    string_to_sign: String,
}

#[derive(Clone)]
pub struct CustomHttpProvider {
    client: reqwest::Client,
    sandbox: Arc<PluginHttpSandbox>,
    base_url: String,
    secrets: Arc<HashMap<String, String>>,
    manifest: Arc<PluginManifest>,
    oauth_token: Arc<Mutex<Option<CachedOauthToken>>>,
    /// 0.4.41：可选 WASM host，channel manifest.security.wasm 配置时启用
    wasm_host: Option<Arc<dyn gate_wasm::WasmHost>>,
    /// channel id（用于 wasm hook 隔离），未传则用 base_url hash
    wasm_channel_id: String,
}

#[derive(Debug, Clone)]
struct CachedOauthToken {
    access_token: String,
    token_type: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
struct OauthTokenResponse {
    access_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct PluginProbeRequest {
    pub method: Method,
    pub url: String,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
    pub model: String,
    pub success_status: Vec<u16>,
    pub max_cost_micros: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct RedactedPluginProbeRequest {
    pub method: Method,
    pub url: String,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
    pub model: String,
    pub success_status: Vec<u16>,
    pub max_cost_micros: Option<i64>,
}

impl CustomHttpProvider {
    pub fn new_with_opts(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        manifest: Value,
        opts: crate::ProviderOpts,
    ) -> ProviderResult<Self> {
        Self::new_with_secret_slots(
            base_url,
            HashMap::from([("primary".to_string(), api_key.into())]),
            manifest,
            opts,
        )
    }

    pub fn new_with_secret_slots(
        base_url: impl Into<String>,
        secrets: HashMap<String, String>,
        manifest: Value,
        opts: crate::ProviderOpts,
    ) -> ProviderResult<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let manifest = PluginManifest::from_value(manifest, &base_url)?;
        let sandbox = Arc::new(PluginHttpSandbox::new(&manifest)?);
        sandbox.validate_endpoint(&base_url, EndpointKind::BaseUrl)?;
        if manifest.auth.strategy == AuthStrategy::OauthClientCredentials {
            sandbox.validate_endpoint(&manifest.auth.oauth.token_url, EndpointKind::OauthToken)?;
        }
        let timeout_ms = manifest.request.timeout_ms.unwrap_or(opts.timeout_ms);
        let client = reqwest::Client::builder()
            .connect_timeout(opts.connect_timeout())
            .timeout(std::time::Duration::from_millis(timeout_ms))
            .dns_resolver(Arc::new(SandboxDnsResolver::new(sandbox.clone())))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ProviderError::Config(e.to_string()))?;
        Ok(Self {
            client,
            sandbox,
            base_url,
            secrets: Arc::new(normalize_secret_slots(secrets)),
            manifest: Arc::new(manifest),
            oauth_token: Arc::new(Mutex::new(None)),
            wasm_host: None,
            wasm_channel_id: String::new(),
        })
    }

    /// 0.4.41：注入 wasm host + channel id，启用 ADR-0003 transform hook 链。
    /// 由 router 在创建 provider 后按 manifest.security.wasm 字段决定是否调用。
    pub fn with_wasm_host(
        mut self,
        host: Arc<dyn gate_wasm::WasmHost>,
        channel_id: impl Into<String>,
    ) -> Self {
        self.wasm_host = Some(host);
        self.wasm_channel_id = channel_id.into();
        self
    }

    pub fn env_secret_slots(channel_code: &str) -> HashMap<String, String> {
        env_secret_slots(channel_code)
    }

    /// 0.4.42: 调 wasm chat_request_transform hook（如配置）。
    /// 失败永不 propagate — fallback policy 内部已降级 identity。
    pub(super) async fn wasm_transform_request(
        &self,
        body: Vec<u8>,
        req: &ChatRequest,
    ) -> Vec<u8> {
        let Some(host) = self.wasm_host.clone() else {
            return body;
        };
        if self.manifest.security.wasm.is_none() {
            return body;
        }
        let ctx = gate_wasm::HookContext {
            channel_id: self.wasm_channel_id.clone(),
            model: req.model.clone(),
            request_id: String::new(),
            metadata: Default::default(),
        };
        let result = gate_wasm::invoke_with_fallback(
            host,
            &self.wasm_channel_id,
            gate_wasm::HookKind::ChatRequest,
            bytes::Bytes::from(body.clone()),
            ctx,
        )
        .await;
        result.to_vec()
    }

    /// 0.4.43: 调 wasm chat_response_transform hook。
    pub(super) async fn wasm_transform_response(&self, body: Vec<u8>, model: &str) -> Vec<u8> {
        let Some(host) = self.wasm_host.clone() else {
            return body;
        };
        if self.manifest.security.wasm.is_none() {
            return body;
        }
        let ctx = gate_wasm::HookContext {
            channel_id: self.wasm_channel_id.clone(),
            model: model.to_string(),
            request_id: String::new(),
            metadata: Default::default(),
        };
        let result = gate_wasm::invoke_with_fallback(
            host,
            &self.wasm_channel_id,
            gate_wasm::HookKind::ChatResponse,
            bytes::Bytes::from(body.clone()),
            ctx,
        )
        .await;
        result.to_vec()
    }

    /// 0.4.44: 调 wasm stream_chunk_transform hook（每 SSE chunk 一次）。
    pub(super) async fn wasm_transform_stream_chunk(
        &self,
        chunk: Vec<u8>,
        model: &str,
    ) -> Vec<u8> {
        let Some(host) = self.wasm_host.clone() else {
            return chunk;
        };
        if self.manifest.security.wasm.is_none() {
            return chunk;
        }
        let ctx = gate_wasm::HookContext {
            channel_id: self.wasm_channel_id.clone(),
            model: model.to_string(),
            request_id: String::new(),
            metadata: Default::default(),
        };
        let result = gate_wasm::invoke_with_fallback(
            host,
            &self.wasm_channel_id,
            gate_wasm::HookKind::StreamChunk,
            bytes::Bytes::from(chunk.clone()),
            ctx,
        )
        .await;
        result.to_vec()
    }

    pub async fn build_probe_request(&self) -> ProviderResult<PluginProbeRequest> {
        let probe = &self.manifest.probe;
        let model = probe
            .model
            .clone()
            .unwrap_or_else(|| "gpt-4o-mini".to_string());
        let req = ChatRequest {
            model: model.clone(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: Some(MessageContent::Text("Hi".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            max_tokens: Some(1),
            temperature: Some(0.0),
            stream: false,
            ..Default::default()
        };
        let ctx = self.request_context_for(&req)?;
        let method = if probe.body.is_some() {
            Method::POST
        } else {
            Method::GET
        };
        let body = match &probe.body {
            Some(template) => {
                let value = render_value(template, &ctx);
                let bytes = serde_json::to_vec(&value)?;
                enforce_size(
                    "plugin probe body",
                    bytes.len(),
                    self.manifest.security.max_request_bytes(),
                )?;
                Some(bytes)
            }
            None => None,
        };
        let url = self.probe_url_with_context(probe, &ctx)?;
        let mut headers = self
            .request_headers_with_context_runtime(
                &ctx,
                &url,
                body.as_deref().unwrap_or_default(),
                method.as_str(),
            )
            .await?;
        if body.is_some() {
            headers
                .entry(reqwest::header::CONTENT_TYPE)
                .or_insert(HeaderValue::from_static("application/json"));
        }
        Ok(PluginProbeRequest {
            method,
            url,
            headers,
            body,
            model,
            success_status: probe.success_status_or_default(),
            max_cost_micros: probe.max_cost_micros,
        })
    }

    pub fn redacted_probe_request(&self, probe: &PluginProbeRequest) -> RedactedPluginProbeRequest {
        RedactedPluginProbeRequest {
            method: probe.method.clone(),
            url: self.sandbox.redact_url(&probe.url),
            headers: self.sandbox.redact_headers(&probe.headers),
            body: probe.body.clone(),
            model: probe.model.clone(),
            success_status: probe.success_status.clone(),
            max_cost_micros: probe.max_cost_micros,
        }
    }

    fn endpoint_url_for(&self, req: &ChatRequest) -> ProviderResult<String> {
        let ctx = self.request_context_for(req)?;
        self.endpoint_url_with_path_and_context(
            self.manifest
                .request
                .path
                .as_deref()
                .unwrap_or(DEFAULT_CHAT_PATH),
            &ctx,
        )
    }

    fn embedding_endpoint_url_for(&self, req: &EmbeddingRequest) -> ProviderResult<String> {
        let ctx = self.embedding_request_context_for(req);
        self.endpoint_url_with_path_and_context(
            self.manifest
                .request
                .embedding_path
                .as_deref()
                .unwrap_or(DEFAULT_EMBEDDINGS_PATH),
            &ctx,
        )
    }

    fn request_method(&self) -> Method {
        match self.manifest.request.method {
            RequestMethod::Get => Method::GET,
            RequestMethod::Post => Method::POST,
        }
    }

    fn endpoint_url_with_path_and_context(
        &self,
        path: &str,
        ctx: &Value,
    ) -> ProviderResult<String> {
        let rendered = render_template_str(path, ctx);
        if rendered.starts_with("http://") || rendered.starts_with("https://") {
            if !self.manifest.security.allow_absolute_chat_path {
                return Err(ProviderError::Config(
                    "plugin request.chat_path must be relative; absolute URLs are disabled by default"
                        .into(),
                ));
            }
            self.sandbox
                .validate_endpoint(&rendered, EndpointKind::AbsolutePath)?;
            return self.url_with_query(rendered, ctx);
        }
        let endpoint = format!("{}{}", self.base_url, slash_path(&rendered));
        self.sandbox
            .validate_endpoint(&endpoint, EndpointKind::BaseUrl)?;
        self.url_with_query(endpoint, ctx)
    }

    fn probe_url_with_context(&self, probe: &ProbeManifest, ctx: &Value) -> ProviderResult<String> {
        let path = probe.path.as_deref().unwrap_or("/models");
        let rendered = render_template_str(path, ctx);
        if rendered.starts_with("http://") || rendered.starts_with("https://") {
            if !self.manifest.security.allow_absolute_chat_path {
                return Err(ProviderError::Config(
                    "plugin probe.path must be relative; absolute URLs are disabled by default"
                        .into(),
                ));
            }
            self.sandbox
                .validate_endpoint(&rendered, EndpointKind::AbsolutePath)?;
            return self.url_with_query(rendered, ctx);
        }
        let endpoint = format!("{}{}", self.base_url, slash_path(&rendered));
        self.sandbox
            .validate_endpoint(&endpoint, EndpointKind::BaseUrl)?;
        self.url_with_query(endpoint, ctx)
    }

    fn url_with_query(&self, endpoint: String, ctx: &Value) -> ProviderResult<String> {
        let mut url = Url::parse(&endpoint)
            .map_err(|e| ProviderError::Config(format!("invalid plugin endpoint URL: {e}")))?;
        let mut rendered_pairs = Vec::new();
        for (name, value) in &self.manifest.request.query {
            if value.is_null() {
                continue;
            }
            if let Some(rendered) = render_template(value, ctx) {
                rendered_pairs.push((name.clone(), rendered));
            }
        }
        if self.manifest.auth.strategy == AuthStrategy::ApiKeyQuery
            && let Some(name) = self.manifest.auth.query_name()
        {
            rendered_pairs.push((
                name.to_string(),
                self.secret_for_slot(self.manifest.auth.secret_slot()),
            ));
        }
        if rendered_pairs.is_empty() {
            return Ok(url.to_string());
        }
        {
            let mut pairs = url.query_pairs_mut();
            for (name, value) in rendered_pairs {
                pairs.append_pair(&name, &value);
            }
        }
        Ok(url.to_string())
    }

    #[cfg(test)]
    fn request_headers_for(&self, req: &ChatRequest) -> ProviderResult<HeaderMap> {
        let body = self.request_json_body(req)?;
        let endpoint = self.endpoint_url_for(req)?;
        self.request_headers_for_parts(req, &endpoint, &body, self.request_method().as_str())
    }

    #[cfg(test)]
    fn request_headers_for_parts(
        &self,
        req: &ChatRequest,
        endpoint: &str,
        body: &[u8],
        method: &str,
    ) -> ProviderResult<HeaderMap> {
        let ctx = self.request_context_for(req)?;
        self.request_headers_with_context(&ctx, endpoint, body, method)
    }

    #[cfg(test)]
    fn embedding_request_headers_for(&self, req: &EmbeddingRequest) -> ProviderResult<HeaderMap> {
        let body = self.embedding_request_json_body(req)?;
        let endpoint = self.embedding_endpoint_url_for(req)?;
        let ctx = self.embedding_request_context_for(req);
        self.request_headers_with_context(&ctx, &endpoint, &body, self.request_method().as_str())
    }

    async fn request_headers_for_parts_runtime(
        &self,
        req: &ChatRequest,
        endpoint: &str,
        body: &[u8],
        method: &str,
    ) -> ProviderResult<HeaderMap> {
        let ctx = self.request_context_for(req)?;
        self.request_headers_with_context_runtime(&ctx, endpoint, body, method)
            .await
    }

    fn request_headers_with_context(
        &self,
        ctx: &Value,
        endpoint: &str,
        body: &[u8],
        method: &str,
    ) -> ProviderResult<HeaderMap> {
        let mut headers = HeaderMap::new();
        self.apply_auth_headers(&mut headers, ctx, endpoint, body, method)?;
        for (k, v) in &self.manifest.request.headers {
            if v.is_null() {
                continue;
            }
            let Some(rendered) = render_template(v, ctx) else {
                continue;
            };
            let name = HeaderName::from_bytes(k.as_bytes())
                .map_err(|e| ProviderError::Config(format!("invalid plugin header {k:?}: {e}")))?;
            let value = HeaderValue::from_str(&rendered).map_err(|e| {
                ProviderError::Config(format!("invalid plugin header value for {k}: {e}"))
            })?;
            headers.insert(name, value);
        }

        Ok(headers)
    }

    async fn request_headers_with_context_runtime(
        &self,
        ctx: &Value,
        endpoint: &str,
        body: &[u8],
        method: &str,
    ) -> ProviderResult<HeaderMap> {
        let mut headers = self.request_headers_with_context(ctx, endpoint, body, method)?;
        if self.manifest.auth.strategy == AuthStrategy::OauthClientCredentials {
            self.apply_oauth_client_credentials_auth_header(&mut headers)
                .await?;
        }
        Ok(headers)
    }

    fn plugin_context(&self) -> Value {
        let primary = self.secret_for_slot("primary");
        let mut secrets = serde_json::Map::new();
        for (slot, value) in self.secrets.iter() {
            secrets.insert(slot.clone(), Value::String(value.clone()));
        }
        json!({
            "api_key": primary,
            "secrets": secrets,
            "aws_secret_key": self.secret_for_slot("aws_secret_key"),
            "aws_session_token": self.secret_for_slot("aws_session_token"),
        })
    }

    fn apply_auth_headers(
        &self,
        headers: &mut HeaderMap,
        ctx: &Value,
        endpoint: &str,
        body: &[u8],
        method: &str,
    ) -> ProviderResult<()> {
        match self.manifest.auth.strategy {
            AuthStrategy::Bearer => {
                let secret = self.secret_for_slot(self.manifest.auth.secret_slot());
                if !secret.is_empty() {
                    insert_header(
                        headers,
                        reqwest::header::AUTHORIZATION,
                        format!("Bearer {secret}"),
                    )?;
                }
            }
            AuthStrategy::ApiKeyHeader => {
                if let Some(name) = self.manifest.auth.header_name() {
                    insert_named_header(
                        headers,
                        name,
                        self.secret_for_slot(self.manifest.auth.secret_slot()),
                    )?;
                }
            }
            AuthStrategy::Basic => {
                let username = self
                    .manifest
                    .auth
                    .username_slot()
                    .map(|slot| self.secret_for_slot(slot))
                    .unwrap_or_default();
                let password = self
                    .manifest
                    .auth
                    .password_slot()
                    .map(|slot| self.secret_for_slot(slot))
                    .unwrap_or_else(|| self.secret_for_slot(self.manifest.auth.secret_slot()));
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(format!("{username}:{password}"));
                insert_header(
                    headers,
                    reqwest::header::AUTHORIZATION,
                    format!("Basic {encoded}"),
                )?;
            }
            AuthStrategy::CustomHeaders => {
                for (name, value) in &self.manifest.auth.headers {
                    if value.is_null() {
                        continue;
                    }
                    if let Some(rendered) = render_template(value, ctx) {
                        insert_named_header(headers, name, rendered)?;
                    }
                }
            }
            AuthStrategy::Hmac => {
                self.apply_hmac_auth_headers(headers, ctx, endpoint, body, method)?;
            }
            AuthStrategy::AwsSigv4 => {
                self.apply_aws_sigv4_auth_headers(headers, endpoint, body, method)?;
            }
            AuthStrategy::OauthClientCredentials => {}
            AuthStrategy::ApiKeyQuery | AuthStrategy::None => {}
        }
        Ok(())
    }

    async fn apply_oauth_client_credentials_auth_header(
        &self,
        headers: &mut HeaderMap,
    ) -> ProviderResult<()> {
        let token = self.oauth_access_token().await?;
        insert_header(
            headers,
            reqwest::header::AUTHORIZATION,
            format!("{} {}", token.token_type, token.access_token),
        )
    }

    async fn oauth_access_token(&self) -> ProviderResult<CachedOauthToken> {
        let mut guard = self.oauth_token.lock().await;
        let now = chrono::Utc::now();
        if let Some(token) = guard.as_ref()
            && token.expires_at > now
        {
            return Ok(token.clone());
        }

        let token = self.fetch_oauth_access_token(now).await?;
        *guard = Some(token.clone());
        Ok(token)
    }

    async fn fetch_oauth_access_token(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> ProviderResult<CachedOauthToken> {
        let oauth = &self.manifest.auth.oauth;
        self.sandbox
            .validate_endpoint(&oauth.token_url, EndpointKind::OauthToken)?;
        let client_id = self.secret_for_slot(&oauth.client_id_slot);
        let client_secret = self.secret_for_slot(&oauth.client_secret_slot);
        if client_id.is_empty() {
            return Err(ProviderError::Config(format!(
                "oauth_client_credentials client id slot '{}' is empty",
                oauth.client_id_slot
            )));
        }
        if client_secret.is_empty() {
            return Err(ProviderError::Config(format!(
                "oauth_client_credentials client secret slot '{}' is empty",
                oauth.client_secret_slot
            )));
        }

        let mut form = vec![
            ("grant_type", "client_credentials".to_string()),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ];
        if let Some(scope) = oauth
            .scope
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            form.push(("scope", scope.to_string()));
        }
        if let Some(audience) = oauth
            .audience
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            form.push(("audience", audience.to_string()));
        }

        let resp = self
            .client
            .post(oauth.token_url.trim())
            .form(&form)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        check_status(&resp)?;
        enforce_response_length_hint(&resp, self.manifest.security.max_response_bytes())?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        let parsed: OauthTokenResponse = resp.json().await.map_err(ProviderError::from)?;
        let access_token = parsed.access_token.unwrap_or_default();
        if access_token.trim().is_empty() {
            return Err(ProviderError::Decode(
                "oauth token response missing access_token".to_string(),
            ));
        }

        let token_type = parsed
            .token_type
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&oauth.token_type)
            .to_string();
        let expires_in = parsed.expires_in.unwrap_or(3600).max(1);
        let effective_ttl = expires_in
            .saturating_sub(oauth.expiry_skew_seconds as i64)
            .max(1);
        Ok(CachedOauthToken {
            access_token,
            token_type,
            expires_at: now + chrono::Duration::seconds(effective_ttl),
        })
    }

    fn apply_hmac_auth_headers(
        &self,
        headers: &mut HeaderMap,
        ctx: &Value,
        endpoint: &str,
        body: &[u8],
        method: &str,
    ) -> ProviderResult<()> {
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let nonce = uuid::Uuid::now_v7().to_string();
        let signature = self.hmac_signature(endpoint, body, method, &timestamp, &nonce, ctx)?;
        let hmac = &self.manifest.auth.hmac;
        insert_named_header(headers, &hmac.timestamp_header, timestamp)?;
        insert_named_header(headers, &hmac.nonce_header, nonce)?;
        insert_named_header(headers, &hmac.signature_header, signature)
    }

    fn hmac_signature(
        &self,
        endpoint: &str,
        body: &[u8],
        method: &str,
        timestamp: &str,
        nonce: &str,
        ctx: &Value,
    ) -> ProviderResult<String> {
        let secret = self.secret_for_slot(self.manifest.auth.secret_slot());
        if secret.is_empty() {
            return Err(ProviderError::Config(format!(
                "hmac auth secret slot '{}' is empty",
                self.manifest.auth.secret_slot()
            )));
        }
        let signed_payload =
            self.hmac_signed_payload(endpoint, body, method, timestamp, nonce, ctx)?;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|e| ProviderError::Config(format!("invalid hmac secret: {e}")))?;
        mac.update(signed_payload.as_bytes());
        let signature = mac.finalize().into_bytes();
        Ok(match self.manifest.auth.hmac.signature_encoding {
            SignatureEncoding::Base64 => {
                base64::engine::general_purpose::STANDARD.encode(signature)
            }
            SignatureEncoding::Hex => hex::encode(signature),
        })
    }

    fn hmac_signed_payload(
        &self,
        endpoint: &str,
        body: &[u8],
        method: &str,
        timestamp: &str,
        nonce: &str,
        request_ctx: &Value,
    ) -> ProviderResult<String> {
        let url = Url::parse(endpoint)
            .map_err(|e| ProviderError::Config(format!("invalid plugin endpoint URL: {e}")))?;
        let body_text = std::str::from_utf8(body).unwrap_or_default();
        let ctx = json!({
            "method": method,
            "path": url.path(),
            "query": url.query().unwrap_or_default(),
            "body": body_text,
            "body_sha256": sha256_hex(body),
            "timestamp": timestamp,
            "nonce": nonce,
            "request": request_ctx.get("request").cloned().unwrap_or(Value::Null),
        });
        Ok(render_template_str(
            &self.manifest.auth.hmac.signed_payload,
            &ctx,
        ))
    }

    fn apply_aws_sigv4_auth_headers(
        &self,
        headers: &mut HeaderMap,
        endpoint: &str,
        body: &[u8],
        method: &str,
    ) -> ProviderResult<()> {
        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        self.apply_aws_sigv4_auth_headers_at(headers, endpoint, body, method, &amz_date, &date)
    }

    fn apply_aws_sigv4_auth_headers_at(
        &self,
        headers: &mut HeaderMap,
        endpoint: &str,
        body: &[u8],
        method: &str,
        amz_date: &str,
        date: &str,
    ) -> ProviderResult<()> {
        let signature = self.aws_sigv4_signature(endpoint, body, method, amz_date, date)?;
        insert_named_header(headers, "x-amz-date", amz_date.to_string())?;
        insert_named_header(headers, "x-amz-content-sha256", sha256_hex(body))?;
        if let Some(token) = self.aws_sigv4_session_token() {
            insert_named_header(headers, "x-amz-security-token", token)?;
        }
        insert_named_header(headers, "authorization", signature.authorization)
    }

    fn aws_sigv4_signature(
        &self,
        endpoint: &str,
        body: &[u8],
        method: &str,
        amz_date: &str,
        date: &str,
    ) -> ProviderResult<AwsSigv4Signature> {
        let conf = &self.manifest.auth.aws_sigv4;
        let access_key = self.secret_for_slot(&conf.access_key_slot);
        let secret_key = self.secret_for_slot(&conf.secret_key_slot);
        if access_key.is_empty() {
            return Err(ProviderError::Config(format!(
                "aws_sigv4 access key slot '{}' is empty",
                conf.access_key_slot
            )));
        }
        if secret_key.is_empty() {
            return Err(ProviderError::Config(format!(
                "aws_sigv4 secret key slot '{}' is empty",
                conf.secret_key_slot
            )));
        }
        let url = Url::parse(endpoint)
            .map_err(|e| ProviderError::Config(format!("invalid plugin endpoint URL: {e}")))?;
        let host = url
            .host_str()
            .ok_or_else(|| ProviderError::Config("plugin endpoint URL missing host".into()))?;
        let host = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        };
        let region = conf
            .region
            .as_deref()
            .map(Cow::Borrowed)
            .or_else(|| infer_aws_region_from_host(&host).map(Cow::Owned))
            .unwrap_or(Cow::Borrowed("us-east-1"));
        let payload_hash = sha256_hex(body);
        let canonical_uri = canonical_uri(&url);
        let canonical_query = canonical_query_string(&url);
        let canonical_headers =
            format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        );
        let credential_scope = format!("{date}/{}/{}/aws4_request", region, conf.service);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signing_key = aws_sigv4_signing_key(&secret_key, date, &region, &conf.service)?;
        let signature = hmac_sha256_hex(&signing_key, string_to_sign.as_bytes())?;
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={access_key}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}"
        );
        Ok(AwsSigv4Signature {
            authorization,
            #[cfg(test)]
            canonical_request,
            #[cfg(test)]
            string_to_sign,
        })
    }

    fn aws_sigv4_session_token(&self) -> Option<String> {
        self.manifest
            .auth
            .aws_sigv4
            .session_token_slot
            .as_deref()
            .map(|slot| self.secret_for_slot(slot))
            .filter(|token| !token.is_empty())
    }

    fn secret_for_slot(&self, slot: &str) -> String {
        let normalized = normalize_secret_slot(slot);
        if normalized == "api_key" {
            return self.secret_for_slot("primary");
        }
        self.secrets
            .get(&normalized)
            .cloned()
            .or_else(|| std::env::var(env_key_for_secret_slot(&normalized)).ok())
            .unwrap_or_default()
    }

    fn request_context_for(&self, req: &ChatRequest) -> ProviderResult<Value> {
        let effective_req = adapt_chat_request(req, self.manifest.preset.adapter)?;
        Ok(request_context(&effective_req, &self.plugin_context()))
    }

    fn embedding_request_context_for(&self, req: &EmbeddingRequest) -> Value {
        embedding_request_context(req, &self.plugin_context())
    }

    #[cfg(test)]
    fn build_body(&self, req: &ChatRequest) -> ProviderResult<Value> {
        self.build_body_with_extra(req, &json!({}))
    }

    fn build_body_with_extra(&self, req: &ChatRequest, extra: &Value) -> ProviderResult<Value> {
        let effective_req = adapt_chat_request(req, self.manifest.preset.adapter)?;
        let ctx = request_context(&effective_req, extra);
        let mut body = match &self.manifest.request.body {
            Some(template) => render_value(template, &ctx),
            None => serde_json::to_value(&effective_req)?,
        };

        if self.manifest.request.force_stream_field {
            set_path(
                &mut body,
                &self.manifest.request.stream_path,
                Value::Bool(effective_req.stream),
            );
        }
        Ok(body)
    }

    fn request_json_body(&self, req: &ChatRequest) -> ProviderResult<Vec<u8>> {
        let body = self.build_body_with_extra(req, &self.plugin_context())?;
        let bytes = serde_json::to_vec(&body)?;
        enforce_size(
            "plugin request body",
            bytes.len(),
            self.manifest.security.max_request_bytes(),
        )?;
        Ok(bytes)
    }

    #[cfg(test)]
    fn build_embedding_body(&self, req: &EmbeddingRequest) -> ProviderResult<Value> {
        self.build_embedding_body_with_extra(req, &json!({}))
    }

    fn build_embedding_body_with_extra(
        &self,
        req: &EmbeddingRequest,
        extra: &Value,
    ) -> ProviderResult<Value> {
        let ctx = embedding_request_context(req, extra);
        match &self.manifest.request.embedding_body {
            Some(template) => Ok(render_value(template, &ctx)),
            None => Ok(serde_json::to_value(req)?),
        }
    }

    fn embedding_request_json_body(&self, req: &EmbeddingRequest) -> ProviderResult<Vec<u8>> {
        let body = self.build_embedding_body_with_extra(req, &self.plugin_context())?;
        let bytes = serde_json::to_vec(&body)?;
        enforce_size(
            "plugin embedding request body",
            bytes.len(),
            self.manifest.security.max_request_bytes(),
        )?;
        Ok(bytes)
    }

    async fn limited_json_response(&self, resp: reqwest::Response) -> ProviderResult<Value> {
        let limit = self.manifest.security.max_response_bytes();
        let mut body = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if body.len().saturating_add(chunk.len()) > limit {
                return Err(ProviderError::Decode(format!(
                    "plugin response body too large: more than {limit} bytes"
                )));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// 0.4.43: 读 body raw bytes，先过 wasm chat_response_transform，再 parse JSON。
    async fn limited_json_response_with_wasm(
        &self,
        resp: reqwest::Response,
        model: &str,
    ) -> ProviderResult<Value> {
        let limit = self.manifest.security.max_response_bytes();
        let mut body = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if body.len().saturating_add(chunk.len()) > limit {
                return Err(ProviderError::Decode(format!(
                    "plugin response body too large: more than {limit} bytes"
                )));
            }
            body.extend_from_slice(&chunk);
        }
        // 走 wasm transform（如配置）；identity passthrough by default
        let body = self.wasm_transform_response(body, model).await;
        Ok(serde_json::from_slice(&body)?)
    }

    async fn limited_error_body(&self, resp: reqwest::Response) -> ProviderResult<String> {
        let limit = self.manifest.security.max_response_bytes().min(64 * 1024);
        let mut body = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if body.len().saturating_add(chunk.len()) > limit {
                return Err(ProviderError::Decode(format!(
                    "plugin error body too large: more than {limit} bytes"
                )));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }

    async fn check_plugin_status(
        &self,
        resp: reqwest::Response,
    ) -> ProviderResult<reqwest::Response> {
        if resp.status().is_success() {
            return Ok(resp);
        }

        let status = resp.status().as_u16();
        let retry_after_ms = retry_after_ms(resp.headers());
        let body = self.limited_error_body(resp).await?;
        Err(self.map_error_response(status, retry_after_ms, &body))
    }

    fn map_error_response(
        &self,
        status: u16,
        retry_after_ms: Option<u64>,
        body: &str,
    ) -> ProviderError {
        let parsed = serde_json::from_str::<Value>(body).ok();
        let status_from_body = parsed
            .as_ref()
            .and_then(|value| {
                self.manifest
                    .error
                    .status_path
                    .as_deref()
                    .and_then(|path| eval_path_value(value, path).ok().flatten())
            })
            .and_then(|value| value_to_u16(&value));
        let effective_status = status_from_body.unwrap_or(status);
        let code = parsed
            .as_ref()
            .and_then(|value| {
                self.manifest
                    .error
                    .code_path
                    .as_deref()
                    .and_then(|path| eval_path_value(value, path).ok().flatten())
                    .or_else(|| eval_path_value(value, "error.code").ok().flatten())
                    .or_else(|| eval_path_value(value, "code").ok().flatten())
                    .or_else(|| eval_path_value(value, "type").ok().flatten())
            })
            .and_then(|value| value_to_string(&value));
        let message = parsed
            .as_ref()
            .and_then(|value| {
                self.manifest
                    .error
                    .message_path
                    .as_deref()
                    .and_then(|path| eval_path_value(value, path).ok().flatten())
                    .or_else(|| eval_path_value(value, "error.message").ok().flatten())
                    .or_else(|| eval_path_value(value, "message").ok().flatten())
                    .or_else(|| eval_path_value(value, "error").ok().flatten())
            })
            .and_then(|value| value_to_string(&value))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                let trimmed = body.trim();
                if trimmed.is_empty() {
                    format!("upstream returned {status}")
                } else {
                    trimmed.chars().take(512).collect()
                }
            });

        let kind = self.classify_error(effective_status, code.as_deref(), &message);
        let retryable = self.error_retryable(effective_status, code.as_deref(), kind);
        let cooldown_ms = self
            .manifest
            .error
            .cooldown_ms
            .or(self.manifest.request.retry.cooldown_ms)
            .or_else(|| {
                retry_after_ms.filter(|_| matches!(kind, NormalizedProviderErrorKind::RateLimit))
            });
        let circuit_breaker_failures = self.manifest.error.circuit_breaker_failures.or(self
            .manifest
            .request
            .retry
            .circuit_breaker_failures);
        let metadata = ProviderErrorMetadata {
            kind,
            retryable,
            cooldown_ms,
            circuit_breaker_failures,
            retry_after_ms,
        };

        ProviderError::Mapped {
            status: Some(effective_status),
            code,
            message,
            metadata,
        }
    }

    fn classify_error(
        &self,
        status: u16,
        code: Option<&str>,
        message: &str,
    ) -> NormalizedProviderErrorKind {
        if status_in(status, &self.manifest.error.auth_status)
            || matches!(status, 401 | 403)
            || code_matches(
                code,
                &["authentication_error", "invalid_api_key", "unauthorized"],
            )
        {
            return NormalizedProviderErrorKind::Authentication;
        }
        if status_in(status, &self.manifest.error.rate_limit_status)
            || status == 429
            || code_matches(
                code,
                &["rate_limit_error", "rate_limited", "too_many_requests"],
            )
        {
            return NormalizedProviderErrorKind::RateLimit;
        }
        if status_in(status, &self.manifest.error.model_not_found_status)
            || status == 404
            || code_matches(
                code,
                &[
                    "model_not_found",
                    "model_not_found_error",
                    "invalid_model",
                    "invalid_request_error",
                ],
            )
        {
            return NormalizedProviderErrorKind::ModelNotFound;
        }
        if code.is_some_and(|code| {
            self.manifest
                .error
                .safety_block_codes
                .iter()
                .any(|c| c == code)
        }) || code_matches(
            code,
            &["content_filter", "policy_violation", "safety_block"],
        ) || message_contains_any(message, &["content filter", "policy", "safety"])
        {
            return NormalizedProviderErrorKind::Policy;
        }
        NormalizedProviderErrorKind::Upstream
    }

    fn error_retryable(
        &self,
        status: u16,
        code: Option<&str>,
        kind: NormalizedProviderErrorKind,
    ) -> bool {
        matches!(kind, NormalizedProviderErrorKind::RateLimit)
            || status >= 500
            || self.manifest.error.retryable_status.contains(&status)
            || self
                .manifest
                .request
                .retry
                .retryable_status
                .contains(&status)
            || code.is_some_and(|code| {
                self.manifest
                    .error
                    .retryable_codes
                    .iter()
                    .chain(self.manifest.request.retry.retryable_codes.iter())
                    .any(|c| c == code)
            })
    }

    fn parse_chat_response(
        &self,
        value: Value,
        requested_model: &str,
    ) -> ProviderResult<ChatResponse> {
        if self.manifest.response.is_openai_compatible() {
            return Ok(serde_json::from_value(value)?);
        }

        let response = &self.manifest.response;
        let id = response
            .id_path
            .as_deref()
            .and_then(|p| eval_path_value(&value, p).ok().flatten())
            .and_then(|v| value_to_string(&v))
            .unwrap_or_else(|| format!("chatcmpl-{}", uuid::Uuid::now_v7()));
        let model = response
            .model_path
            .as_deref()
            .and_then(|p| eval_path_value(&value, p).ok().flatten())
            .and_then(|v| value_to_string(&v))
            .unwrap_or_else(|| requested_model.to_string());
        let content = response
            .content_path
            .as_deref()
            .and_then(|p| eval_path_value(&value, p).ok().flatten())
            .and_then(|v| value_to_string(&v))
            .unwrap_or_default();
        let reasoning_content = response
            .reasoning_content_path
            .as_deref()
            .and_then(|p| eval_path_value(&value, p).ok().flatten())
            .and_then(|v| value_to_string(&v));
        let finish_reason = response
            .finish_reason_path
            .as_deref()
            .and_then(|p| eval_path_value(&value, p).ok().flatten())
            .and_then(|v| value_to_string(&v))
            .and_then(|s| map_finish_reason(&s));
        let tool_calls = response
            .tool_calls_path
            .as_deref()
            .and_then(|p| eval_path_value(&value, p).ok().flatten())
            .map(serde_json::from_value::<Vec<ToolCall>>)
            .transpose()?;
        let request_id = response
            .request_id_path
            .as_deref()
            .and_then(|p| eval_path_value(&value, p).ok().flatten())
            .and_then(|v| value_to_string(&v));
        let upstream_metadata = response
            .metadata_path
            .as_deref()
            .and_then(|p| eval_path_value(&value, p).ok().flatten());
        let usage = response.usage.extract(&value)?;
        let content = merge_reasoning_content(content, reasoning_content);

        Ok(ChatResponse {
            id,
            model,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: Role::Assistant,
                    content: if content.is_empty() {
                        None
                    } else {
                        Some(MessageContent::Text(content))
                    },
                    name: None,
                    tool_calls,
                    tool_call_id: None,
                },
                finish_reason,
            }],
            usage,
            request_id,
            upstream_metadata,
        })
    }

    fn parse_embedding_response(
        &self,
        value: Value,
        requested_model: &str,
    ) -> ProviderResult<EmbeddingResponse> {
        if self.manifest.embedding_response.is_openai_compatible() {
            return Ok(serde_json::from_value(value)?);
        }

        let response = &self.manifest.embedding_response;
        let object = response
            .object_path
            .as_deref()
            .and_then(|p| eval_path_value(&value, p).ok().flatten())
            .and_then(|v| value_to_string(&v))
            .unwrap_or_else(|| "list".to_string());
        let model = response
            .model_path
            .as_deref()
            .and_then(|p| eval_path_value(&value, p).ok().flatten())
            .and_then(|v| value_to_string(&v))
            .unwrap_or_else(|| requested_model.to_string());
        let data_value = response
            .data_path
            .as_deref()
            .and_then(|p| eval_path_value(&value, p).ok().flatten())
            .unwrap_or_else(|| value.get("data").cloned().unwrap_or_else(|| value.clone()));
        let items = data_value.as_array().ok_or_else(|| {
            ProviderError::Decode("plugin embedding_response.data_path is not an array".into())
        })?;
        let mut data = Vec::with_capacity(items.len());
        for (idx, item) in items.iter().enumerate() {
            let embedding_value = response
                .embedding_path
                .as_deref()
                .and_then(|p| eval_path_value(item, p).ok().flatten())
                .unwrap_or_else(|| {
                    item.get("embedding")
                        .cloned()
                        .unwrap_or_else(|| item.clone())
                });
            let embedding = parse_embedding_vector(&embedding_value)?;
            let index = response
                .index_path
                .as_deref()
                .and_then(|p| eval_path_value(item, p).ok().flatten())
                .and_then(|v| value_to_u32(&v))
                .unwrap_or(idx as u32);
            data.push(EmbeddingData {
                object: "embedding".to_string(),
                index,
                embedding,
            });
        }

        let usage = response.usage.extract(&value)?;
        let prompt_tokens = usage.prompt_tokens;
        let total_tokens = if usage.total_tokens > 0 {
            usage.total_tokens
        } else {
            prompt_tokens
        };
        Ok(EmbeddingResponse {
            object,
            data,
            model,
            usage: EmbeddingUsage {
                prompt_tokens,
                total_tokens,
            },
        })
    }
}

#[async_trait]
impl Provider for CustomHttpProvider {
    fn name(&self) -> &'static str {
        "plugin"
    }

    async fn chat(&self, mut req: ChatRequest) -> ProviderResult<ChatResponse> {
        match self.fastpath_kind() {
            Some(kind @ ProviderPresetKind::Openai) => {
                if let Some(result) = self
                    .run_fastpath(kind, "chat", self.fastpath_openai_chat(req.clone()))
                    .await
                {
                    return result;
                }
                // panic 已经被 catch_unwind 抓住，降级到 manifest runtime
            }
            Some(kind @ ProviderPresetKind::AnthropicMessages) => {
                if let Some(result) = self
                    .run_fastpath(kind, "chat", self.fastpath_anthropic_chat(req.clone()))
                    .await
                {
                    return result;
                }
            }
            Some(kind @ ProviderPresetKind::AzureOpenai) => {
                if let Some(result) = self
                    .run_fastpath(kind, "chat", self.fastpath_azure_chat(req.clone()))
                    .await
                {
                    return result;
                }
            }
            Some(kind @ ProviderPresetKind::BedrockConverse) => {
                if let Some(result) = self
                    .run_fastpath(kind, "chat", self.fastpath_bedrock_chat(req.clone()))
                    .await
                {
                    return result;
                }
            }
            _ => {}
        }
        req.stream = false;
        let body = self.request_json_body(&req)?;
        let body = self.wasm_transform_request(body, &req).await;
        let endpoint = self.endpoint_url_for(&req)?;
        let method = self.request_method();
        let mut headers = self
            .request_headers_for_parts_runtime(&req, &endpoint, &body, method.as_str())
            .await?;
        headers
            .entry(reqwest::header::CONTENT_TYPE)
            .or_insert(HeaderValue::from_static("application/json"));
        let resp = self
            .client
            .request(method, endpoint)
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        let resp = self.check_plugin_status(resp).await?;
        enforce_response_length_hint(&resp, self.manifest.security.max_response_bytes())?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        let body = self.limited_json_response_with_wasm(resp, &req.model).await?;
        self.parse_chat_response(body, &req.model)
    }

    async fn chat_stream(
        &self,
        mut req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        match self.fastpath_kind() {
            Some(kind @ ProviderPresetKind::Openai) => {
                if let Some(result) = self
                    .run_fastpath(
                        kind,
                        "chat_stream",
                        self.fastpath_openai_chat_stream(req.clone()),
                    )
                    .await
                {
                    return result;
                }
            }
            Some(kind @ ProviderPresetKind::AnthropicMessages) => {
                if let Some(result) = self
                    .run_fastpath(
                        kind,
                        "chat_stream",
                        self.fastpath_anthropic_chat_stream(req.clone()),
                    )
                    .await
                {
                    return result;
                }
            }
            Some(kind @ ProviderPresetKind::AzureOpenai) => {
                if let Some(result) = self
                    .run_fastpath(
                        kind,
                        "chat_stream",
                        self.fastpath_azure_chat_stream(req.clone()),
                    )
                    .await
                {
                    return result;
                }
            }
            _ => {}
        }
        req.stream = true;
        let body = self.request_json_body(&req)?;
        let endpoint = self.endpoint_url_for(&req)?;
        let method = self.request_method();
        let mut headers = self
            .request_headers_for_parts_runtime(&req, &endpoint, &body, method.as_str())
            .await?;
        headers
            .entry(reqwest::header::CONTENT_TYPE)
            .or_insert(HeaderValue::from_static("application/json"));
        let resp = self
            .client
            .request(method, endpoint)
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        let resp = self.check_plugin_status(resp).await?;
        enforce_response_length_hint(&resp, self.manifest.security.max_response_bytes())?;

        let mapper = StreamMapper {
            stream: self.manifest.stream.clone(),
            fallback_id: format!("chatcmpl-{}", uuid::Uuid::now_v7()),
            fallback_model: req.model.clone(),
            max_response_bytes: self.manifest.security.max_response_bytes(),
            max_sse_event_bytes: self.manifest.security.max_sse_event_bytes(),
        };
        Ok(normalize_plugin_sse(resp.bytes_stream(), mapper).boxed())
    }
}

#[async_trait]
impl EmbeddingProvider for CustomHttpProvider {
    fn name(&self) -> &'static str {
        "plugin"
    }

    async fn embed(&self, req: EmbeddingRequest) -> ProviderResult<EmbeddingResponse> {
        match self.fastpath_kind() {
            Some(kind @ ProviderPresetKind::Openai) => {
                if let Some(result) = self
                    .run_fastpath(kind, "embed", self.fastpath_openai_embed(req.clone()))
                    .await
                {
                    return result;
                }
            }
            Some(kind @ ProviderPresetKind::AzureOpenai) => {
                if let Some(result) = self
                    .run_fastpath(kind, "embed", self.fastpath_azure_embed(req.clone()))
                    .await
                {
                    return result;
                }
            }
            _ => {}
        }
        let body = self.embedding_request_json_body(&req)?;
        let endpoint = self.embedding_endpoint_url_for(&req)?;
        let method = self.request_method();
        let ctx = self.embedding_request_context_for(&req);
        let mut headers = self
            .request_headers_with_context_runtime(&ctx, &endpoint, &body, method.as_str())
            .await?;
        headers
            .entry(reqwest::header::CONTENT_TYPE)
            .or_insert(HeaderValue::from_static("application/json"));
        let resp = self
            .client
            .request(method, endpoint)
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|e| self.sandbox.reqwest_error(e))?;
        self.sandbox.validate_response_peer(&resp)?;
        let resp = self.check_plugin_status(resp).await?;
        enforce_response_length_hint(&resp, self.manifest.security.max_response_bytes())?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        let body = self.limited_json_response(resp).await?;
        self.parse_embedding_response(body, &req.model)
    }
}

#[cfg(test)]
mod tests;
