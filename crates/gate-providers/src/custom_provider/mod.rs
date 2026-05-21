//! Runtime-configurable HTTP provider plugin.
//!
//! A `provider_type` of `plugin` / `custom` / `http` uses channel `model_mapping`
//! as the plugin manifest. The manifest can reshape requests, map strange JSON
//! responses, and normalize arbitrary SSE frames back into OpenAI-compatible chunks.

mod replay;
mod sandbox;
mod secrets;
mod sigv4;

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
use hmac::{Hmac, Mac};
pub use replay::replay_plugin_sse;
use replay::{StreamMapper, merge_reasoning_content, normalize_plugin_sse};
use reqwest::Method;
use reqwest::Url;
use reqwest::header::{CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue};
use sandbox::{EndpointKind, PluginHttpSandbox, SandboxDnsResolver};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use std::borrow::Cow;
use std::collections::HashMap;
#[cfg(test)]
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

pub(super) type HmacSha256 = Hmac<Sha256>;

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
        })
    }

    pub fn env_secret_slots(channel_code: &str) -> HashMap<String, String> {
        env_secret_slots(channel_code)
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
    fn fastpath_kind(&self) -> Option<ProviderPresetKind> {
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
    async fn run_fastpath<T>(
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
    async fn fastpath_openai_chat(&self, mut req: ChatRequest) -> ProviderResult<ChatResponse> {
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

    async fn fastpath_openai_chat_stream(
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

    async fn fastpath_openai_embed(
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
    async fn fastpath_anthropic_chat(&self, req: ChatRequest) -> ProviderResult<ChatResponse> {
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

    async fn fastpath_anthropic_chat_stream(
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
    fn azure_chat_url(&self, model: &str) -> String {
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

    fn azure_embeddings_url(&self, model: &str) -> String {
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

    async fn fastpath_azure_chat(&self, mut req: ChatRequest) -> ProviderResult<ChatResponse> {
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

    async fn fastpath_azure_chat_stream(
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

    async fn fastpath_azure_embed(
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
            _ => {}
        }
        req.stream = false;
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
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        let body = self.limited_json_response(resp).await?;
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

fn request_context(req: &ChatRequest, extra: &Value) -> Value {
    let last_user = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User)
        .map(ChatMessage::content_text)
        .unwrap_or_default();
    let mut ctx = json!({
        "request": req,
        "extra": req.extra,
        "model": req.model,
        "messages": req.messages,
        "last_user_message": last_user,
        "stream": req.stream,
        "temperature": req.temperature,
        "top_p": req.top_p,
        "max_tokens": req.max_tokens,
        "tools": req.tools.clone(),
        "tool_choice": req.tool_choice.clone(),
        "metadata": req.extra.get("metadata").cloned().unwrap_or(Value::Null),
    });
    if let (Some(dst), Some(src)) = (ctx.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    ctx
}

fn embedding_request_context(req: &EmbeddingRequest, extra: &Value) -> Value {
    let input_texts = match &req.input {
        EmbeddingInput::Single(value) => vec![value.clone()],
        EmbeddingInput::Multiple(values) => values.clone(),
    };
    let mut ctx = json!({
        "request": req,
        "model": req.model,
        "input": req.input,
        "input_texts": input_texts,
        "encoding_format": req.encoding_format,
        "dimensions": req.dimensions,
        "metadata": Value::Null,
        "extra": Value::Null,
    });
    if let (Some(dst), Some(src)) = (ctx.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    ctx
}

fn render_value(template: &Value, ctx: &Value) -> Value {
    render_value_optional(template, ctx)
        .map(|rendered| rendered.value)
        .unwrap_or(Value::Null)
}

struct RenderedValue {
    value: Value,
    conditional: bool,
}

fn render_value_optional(template: &Value, ctx: &Value) -> Option<RenderedValue> {
    match template {
        Value::String(s) => {
            if let Some(path) = whole_placeholder(s) {
                let value = get_path(ctx, path)?;
                if is_empty_placeholder_value(value) {
                    None
                } else {
                    Some(RenderedValue {
                        value: value.clone(),
                        conditional: true,
                    })
                }
            } else {
                Some(RenderedValue {
                    value: Value::String(render_template_str(s, ctx)),
                    conditional: false,
                })
            }
        }
        Value::Array(arr) => {
            let mut conditional = false;
            let mut values = Vec::with_capacity(arr.len());
            for value in arr {
                match render_value_optional(value, ctx) {
                    Some(rendered) => {
                        conditional |= rendered.conditional;
                        values.push(rendered.value);
                    }
                    None => conditional = true,
                }
            }
            if conditional && values.is_empty() {
                None
            } else {
                Some(RenderedValue {
                    value: Value::Array(values),
                    conditional,
                })
            }
        }
        Value::Object(obj) => {
            let mut conditional = false;
            let mut values = serde_json::Map::new();
            for (key, value) in obj {
                match render_value_optional(value, ctx) {
                    Some(rendered) => {
                        conditional |= rendered.conditional;
                        values.insert(key.clone(), rendered.value);
                    }
                    None => conditional = true,
                }
            }
            if conditional && values.is_empty() {
                None
            } else {
                Some(RenderedValue {
                    value: Value::Object(values),
                    conditional,
                })
            }
        }
        other => Some(RenderedValue {
            value: other.clone(),
            conditional: false,
        }),
    }
}

fn render_template(template: &Value, ctx: &Value) -> Option<String> {
    let rendered = render_value_optional(template, ctx)?.value;
    if is_empty_placeholder_value(&rendered) {
        None
    } else {
        value_to_string(&rendered)
    }
}

fn is_empty_placeholder_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

fn render_template_str(template: &str, ctx: &Value) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let (head, after_start) = rest.split_at(start);
        out.push_str(head);
        let after_start = &after_start[2..];
        let Some(end) = after_start.find("}}") else {
            out.push_str("{{");
            out.push_str(after_start);
            return out;
        };
        let (expr, after_expr) = after_start.split_at(end);
        let path = expr.trim();
        if let Some(v) = get_path(ctx, path) {
            out.push_str(&value_to_string(v).unwrap_or_else(|| v.to_string()));
        }
        rest = &after_expr[2..];
    }
    out.push_str(rest);
    out
}

pub(super) fn enforce_size(name: &str, actual: usize, limit: usize) -> ProviderResult<()> {
    if actual > limit {
        return Err(ProviderError::Decode(format!(
            "{name} too large: {actual} bytes > {limit} bytes"
        )));
    }
    Ok(())
}

/// 从 `catch_unwind` 拿到的 `Box<dyn Any + Send>` payload 里提取人话信息。
/// panic payload 通常是 `&'static str` 或 `String`；其他类型显示 `<non-string panic>`.
fn panic_message(payload: &Box<dyn std::any::Any + Send + 'static>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<non-string panic>".to_string()
}

fn enforce_response_length_hint(resp: &reqwest::Response, limit: usize) -> ProviderResult<()> {
    let Some(value) = resp.headers().get(CONTENT_LENGTH) else {
        return Ok(());
    };
    let Some(len) = value.to_str().ok().and_then(|s| s.parse::<usize>().ok()) else {
        return Ok(());
    };
    enforce_size("plugin response body", len, limit)
}

fn insert_named_header(headers: &mut HeaderMap, name: &str, value: String) -> ProviderResult<()> {
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|e| ProviderError::Config(format!("invalid plugin header {name:?}: {e}")))?;
    insert_header(headers, name, value)
}

fn insert_header(headers: &mut HeaderMap, name: HeaderName, value: String) -> ProviderResult<()> {
    let value = HeaderValue::from_str(&value)
        .map_err(|e| ProviderError::Config(format!("invalid plugin header value: {e}")))?;
    headers.insert(name, value);
    Ok(())
}

fn whole_placeholder(s: &str) -> Option<&str> {
    let trimmed = s.trim();
    if trimmed.starts_with("{{") && trimmed.ends_with("}}") && trimmed.matches("{{").count() == 1 {
        Some(
            trimmed
                .trim_start_matches("{{")
                .trim_end_matches("}}")
                .trim(),
        )
    } else {
        None
    }
}

fn slash_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
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

fn set_path(value: &mut Value, path: &str, new_value: Value) {
    let segments: Vec<_> = path.split('.').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        *value = new_value;
        return;
    }
    let mut cur = value;
    for segment in &segments[..segments.len() - 1] {
        if !cur.is_object() {
            *cur = json!({});
        }
        cur = cur
            .as_object_mut()
            .expect("object just created")
            .entry((*segment).to_string())
            .or_insert_with(|| json!({}));
    }
    if !cur.is_object() {
        *cur = json!({});
    }
    cur.as_object_mut()
        .expect("object just created")
        .insert(segments[segments.len() - 1].to_string(), new_value);
}

pub(super) fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

fn value_to_u16(v: &Value) -> Option<u16> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|value| u16::try_from(value).ok()),
        Value::String(s) => s.trim().parse::<u16>().ok(),
        _ => None,
    }
}

fn value_to_u32(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(s) => s.trim().parse::<u32>().ok(),
        _ => None,
    }
}

fn parse_embedding_vector(value: &Value) -> ProviderResult<Vec<f32>> {
    let arr = value
        .as_array()
        .ok_or_else(|| ProviderError::Decode("plugin embedding value is not an array".into()))?;
    arr.iter()
        .map(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<f64>().ok()))
                .map(|n| n as f32)
                .ok_or_else(|| {
                    ProviderError::Decode("plugin embedding vector contains non-number".into())
                })
        })
        .collect()
}

fn retry_after_ms(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| seconds.saturating_mul(1000))
}

fn status_in(status: u16, statuses: &[u16]) -> bool {
    statuses.contains(&status)
}

fn code_matches(code: Option<&str>, values: &[&str]) -> bool {
    let Some(code) = code else {
        return false;
    };
    values.iter().any(|value| code.eq_ignore_ascii_case(value))
}

fn message_contains_any(message: &str, needles: &[&str]) -> bool {
    let message = message.to_ascii_lowercase();
    needles.iter().any(|needle| message.contains(needle))
}

pub(super) fn map_role(s: &str) -> Option<Role> {
    match s {
        "system" => Some(Role::System),
        "user" => Some(Role::User),
        "assistant" | "bot" | "model" => Some(Role::Assistant),
        "tool" => Some(Role::Tool),
        _ => None,
    }
}

pub(super) fn map_finish_reason(s: &str) -> Option<FinishReason> {
    match s {
        "stop" | "stopped" | "stop_sequence" | "end_turn" | "done" => Some(FinishReason::Stop),
        "length" | "max_tokens" => Some(FinishReason::Length),
        "tool_calls" | "tool_use" => Some(FinishReason::ToolCalls),
        "content_filter" | "safety" => Some(FinishReason::ContentFilter),
        "" => None,
        _ => Some(FinishReason::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_manifest::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_MAX_SSE_EVENT_BYTES};
    use futures::StreamExt;

    fn make_req(stream: bool) -> ChatRequest {
        ChatRequest {
            model: "odd-model".into(),
            messages: vec![ChatMessage::text(Role::User, "Hi plugin")],
            max_tokens: Some(16),
            stream,
            ..Default::default()
        }
    }

    #[test]
    fn request_template_preserves_native_json_values() {
        let manifest = json!({
            "request": {
                "body": {
                    "m": "{{model}}",
                    "prompt": "{{last_user_message}}",
                    "streaming": "{{stream}}",
                    "limit": "{{max_tokens}}"
                }
            }
        });
        let provider = CustomHttpProvider::new_with_opts(
            "http://x",
            "k",
            manifest,
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let body = provider.build_body(&make_req(true)).unwrap();
        assert_eq!(body["m"], "odd-model");
        assert_eq!(body["prompt"], "Hi plugin");
        assert_eq!(body["streaming"], true);
        assert_eq!(body["limit"], 16);
    }

    #[test]
    fn request_template_supports_tools_tool_choice_metadata_and_prunes_empty_fields() {
        let manifest = json!({
            "plugin": {
                "version": 1,
                "request": {
                    "path": "/deployments/{{metadata.deployment}}/chat",
                    "query": {
                        "tenant": "{{metadata.tenant}}",
                        "missing": "{{metadata.missing}}"
                    },
                    "headers": {
                        "X-Tenant": "{{metadata.tenant}}",
                        "X-Missing": "{{metadata.missing}}"
                    },
                    "body": {
                        "model": "{{model}}",
                        "messages": "{{messages}}",
                        "tools": "{{tools}}",
                        "tool_choice": "{{tool_choice}}",
                        "metadata": "{{metadata}}",
                        "drop_null": "{{metadata.missing}}",
                        "drop_empty_array": "{{request.parallel_tool_calls}}",
                        "nested": {
                            "keep": "literal",
                            "drop": "{{metadata.missing}}"
                        }
                    }
                }
            }
        });
        let provider = CustomHttpProvider::new_with_opts(
            "https://private.example/v1",
            "k",
            manifest,
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let mut req = make_req(false);
        req.tools = Some(vec![ToolDef {
            r#type: "function".into(),
            function: FunctionDef {
                name: "lookup".into(),
                description: Some("Lookup docs".into()),
                parameters: Some(json!({"type": "object"})),
            },
        }]);
        req.tool_choice = Some(json!({"type": "function", "function": {"name": "lookup"}}));
        req.extra.insert(
            "metadata".into(),
            json!({ "tenant": "acme", "deployment": "private-deploy" }),
        );

        assert_eq!(
            provider.endpoint_url_for(&req).unwrap(),
            "https://private.example/v1/deployments/private-deploy/chat?tenant=acme"
        );
        let headers = provider.request_headers_for(&req).unwrap();
        assert_eq!(headers.get("x-tenant").unwrap(), "acme");
        assert!(headers.get("x-missing").is_none());

        let body = provider.build_body(&req).unwrap();
        assert_eq!(body["model"], "odd-model");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["tools"][0]["function"]["name"], "lookup");
        assert_eq!(body["tool_choice"]["function"]["name"], "lookup");
        assert_eq!(body["metadata"]["tenant"], "acme");
        assert!(body.get("drop_null").is_none());
        assert!(body.get("drop_empty_array").is_none());
        assert_eq!(body["nested"], json!({ "keep": "literal" }));
    }

    #[test]
    fn openai_compatible_preset_expands_defaults_and_usage_stream_options() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.deepseek.com/v1",
            "sk-test",
            json!({ "plugin": { "preset": { "provider": "deepseek" } } }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        assert_eq!(
            provider.endpoint_url_for(&make_req(true)).unwrap(),
            "https://api.deepseek.com/v1/chat/completions"
        );
        let body = provider.build_body(&make_req(true)).unwrap();
        assert_eq!(body["model"], "odd-model");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert!(provider.manifest.response.is_openai_compatible());
        assert!(provider.manifest.stream.is_openai_compatible());
    }

    #[test]
    fn vertex_openai_preset_targets_openai_compatible_vertex_endpoint() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://aiplatform.googleapis.com/v1/projects/demo/locations/us-central1/endpoints/openapi",
            "vertex-token",
            json!({ "plugin": { "preset": { "provider": "vertex_openai" } } }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let req = make_req(false);
        assert_eq!(
            provider.endpoint_url_for(&req).unwrap(),
            "https://aiplatform.googleapis.com/v1/projects/demo/locations/us-central1/endpoints/openapi/chat/completions"
        );
        let headers = provider.request_headers_for(&req).unwrap();
        assert_eq!(headers.get("authorization").unwrap(), "Bearer vertex-token");
        assert_eq!(
            provider
                .embedding_endpoint_url_for(&EmbeddingRequest {
                    model: "text-embedding-3-small".into(),
                    input: EmbeddingInput::Single("hello".into()),
                    encoding_format: None,
                    dimensions: None,
                })
                .unwrap(),
            "https://aiplatform.googleapis.com/v1/projects/demo/locations/us-central1/endpoints/openapi/embeddings"
        );
        assert!(provider.manifest.response.is_openai_compatible());
        assert!(provider.manifest.stream.is_openai_compatible());
    }

    #[test]
    fn azure_preset_templates_deployment_path_and_api_key_header() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://example.openai.azure.com",
            "azure-key",
            json!({
                "plugin": {
                    "preset": { "provider": "azure_openai", "api_version": "2024-02-15-preview" }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let req = make_req(false);
        let body = provider.build_body(&req).unwrap();
        assert_eq!(body["model"], "odd-model");
        assert_eq!(
            provider.endpoint_url_for(&req).unwrap(),
            "https://example.openai.azure.com/openai/deployments/odd-model/chat/completions?api-version=2024-02-15-preview"
        );
        assert_eq!(
            provider
                .embedding_endpoint_url_for(&EmbeddingRequest {
                    model: "odd-model".into(),
                    input: EmbeddingInput::Single("hello".into()),
                    encoding_format: None,
                    dimensions: None,
                })
                .unwrap(),
            "https://example.openai.azure.com/openai/deployments/odd-model/embeddings?api-version=2024-02-15-preview"
        );
        let headers = provider.request_headers_for(&req).unwrap();
        assert_eq!(headers.get("api-key").unwrap(), "azure-key");
        assert!(headers.get("authorization").is_none());
    }

    #[test]
    fn anthropic_preset_adapts_openai_request_to_messages_api() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.anthropic.com",
            "anthropic-key",
            json!({ "plugin": { "preset": { "provider": "anthropic_messages" } } }),
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let req = ChatRequest {
            model: "claude-sonnet".into(),
            messages: vec![
                ChatMessage::text(Role::System, "You are terse"),
                ChatMessage::text(Role::User, "Hi"),
            ],
            max_tokens: Some(32),
            stream: true,
            ..Default::default()
        };

        assert_eq!(
            provider.endpoint_url_for(&req).unwrap(),
            "https://api.anthropic.com/v1/messages"
        );
        let headers = provider.request_headers_for(&req).unwrap();
        assert_eq!(headers.get("x-api-key").unwrap(), "anthropic-key");
        assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");
        assert!(headers.get("authorization").is_none());
        let body = provider.build_body(&req).unwrap();
        assert_eq!(body["model"], "claude-sonnet");
        assert_eq!(body["max_tokens"], 32);
        assert_eq!(body["system"], "You are terse");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hi");
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn plugin_auth_api_key_query_appends_secret_to_url() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com/v1",
            "query-key",
            json!({
                "plugin": {
                    "version": 1,
                    "auth": { "strategy": "api_key_query", "query_name": "key" },
                    "request": {
                        "path": "/private/chat",
                        "query": { "model": "{{model}}" }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let url = provider.endpoint_url_for(&make_req(false)).unwrap();
        assert_eq!(
            url,
            "https://api.example.com/v1/private/chat?model=odd-model&key=query-key"
        );
        let headers = provider.request_headers_for(&make_req(false)).unwrap();
        assert!(headers.get("authorization").is_none());
    }

    #[test]
    fn embedding_request_template_uses_embedding_path_body_and_auth() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com/v1",
            "embed-key",
            json!({
                "plugin": {
                    "version": 1,
                    "capabilities": { "embeddings": true },
                    "auth": { "strategy": "api_key_header", "header_name": "X-Embed-Key" },
                    "request": {
                        "embedding_path": "/private/embed/{{model}}",
                        "query": { "dims": "{{dimensions}}" },
                        "embedding_body": {
                            "modelName": "{{model}}",
                            "texts": "{{input_texts}}",
                            "format": "{{encoding_format}}",
                            "dimensions": "{{dimensions}}"
                        }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let req = EmbeddingRequest {
            model: "embed-model".into(),
            input: EmbeddingInput::Multiple(vec!["hello".into(), "world".into()]),
            encoding_format: Some("float".into()),
            dimensions: Some(3),
        };

        assert_eq!(
            provider.embedding_endpoint_url_for(&req).unwrap(),
            "https://api.example.com/v1/private/embed/embed-model?dims=3"
        );
        let body = provider.build_embedding_body(&req).unwrap();
        assert_eq!(body["modelName"], "embed-model");
        assert_eq!(body["texts"], json!(["hello", "world"]));
        assert_eq!(body["format"], "float");
        assert_eq!(body["dimensions"], 3);
        let headers = provider.embedding_request_headers_for(&req).unwrap();
        assert_eq!(headers.get("x-embed-key").unwrap(), "embed-key");
    }

    #[test]
    fn custom_embedding_response_mapper_normalizes_vendor_vectors() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com/v1",
            "embed-key",
            json!({
                "plugin": {
                    "version": 1,
                    "embedding_response": {
                        "openai_compatible": false,
                        "data_path": "result.vectors",
                        "embedding_path": "values",
                        "index_path": "position",
                        "model_path": "result.model",
                        "usage": {
                            "prompt_tokens_path": "usage.input_tokens",
                            "total_tokens_path": "usage.total_tokens"
                        }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let response = provider
            .parse_embedding_response(
                json!({
                    "result": {
                        "model": "vendor-embed",
                        "vectors": [
                            { "position": 1, "values": [0.1, "0.2"] },
                            { "position": 2, "values": [0.3, 0.4] }
                        ]
                    },
                    "usage": { "input_tokens": 7, "total_tokens": 7 }
                }),
                "fallback-model",
            )
            .unwrap();

        assert_eq!(response.object, "list");
        assert_eq!(response.model, "vendor-embed");
        assert_eq!(response.data[0].index, 1);
        assert_eq!(response.data[0].embedding, vec![0.1, 0.2]);
        assert_eq!(response.usage.total_tokens, 7);
    }

    #[test]
    fn plugin_auth_basic_and_custom_headers_use_secret_slots() {
        // SAFETY: unit test only needs process env for a synthetic plugin secret.
        unsafe {
            std::env::set_var("KOOIX_PLUGIN_SECRET_USER", "basic-user");
        }

        let basic = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "basic-pass",
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "basic",
                        "username_slot": "user",
                        "password_slot": "primary"
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let headers = basic.request_headers_for(&make_req(false)).unwrap();
        assert_eq!(
            headers.get("authorization").unwrap(),
            "Basic YmFzaWMtdXNlcjpiYXNpYy1wYXNz"
        );

        let custom = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "primary-key",
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "custom_headers",
                        "headers": {
                            "X-Api-Key": "{{api_key}}",
                            "X-Model": "{{model}}"
                        }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let headers = custom.request_headers_for(&make_req(false)).unwrap();
        assert_eq!(headers.get("x-api-key").unwrap(), "primary-key");
        assert_eq!(headers.get("x-model").unwrap(), "odd-model");
        assert!(headers.get("authorization").is_none());
    }

    #[test]
    fn plugin_auth_uses_explicit_secret_slot_map() {
        let provider = CustomHttpProvider::new_with_secret_slots(
            "https://api.example.com",
            HashMap::from([
                ("primary".to_string(), "primary-key".to_string()),
                ("Alt-Key".to_string(), "alt-key".to_string()),
            ]),
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "api_key_header",
                        "header_name": "X-Alt-Key",
                        "secret_slot": "alt-key"
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let headers = provider.request_headers_for(&make_req(false)).unwrap();
        assert_eq!(headers.get("x-alt-key").unwrap(), "alt-key");
        assert!(
            headers
                .get("x-alt-key")
                .is_some_and(|value| value != "primary-key")
        );
    }

    #[test]
    fn plugin_env_secret_slots_include_named_plugin_secrets() {
        // SAFETY: unit test owns synthetic plugin env names.
        unsafe {
            std::env::set_var("KOOIX_PLUGIN_SECRET_CLIENT_ID", "env-client");
            std::env::set_var("KOOIX_PLUGIN_SECRET_CLIENT_SECRET", "env-secret");
            std::env::set_var("AWS_SECRET_ACCESS_KEY", "env-aws-secret");
        }

        let secrets = CustomHttpProvider::env_secret_slots("missing-env-channel");
        assert_eq!(
            secrets.get("client_id").map(String::as_str),
            Some("env-client")
        );
        assert_eq!(
            secrets.get("client_secret").map(String::as_str),
            Some("env-secret")
        );
        assert_eq!(
            secrets.get("aws_secret_key").map(String::as_str),
            Some("env-aws-secret")
        );

        let provider = CustomHttpProvider::new_with_secret_slots(
            "https://api.example.com",
            secrets,
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "basic",
                        "username_slot": "client_id",
                        "password_slot": "client_secret"
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let headers = provider.request_headers_for(&make_req(false)).unwrap();
        assert_eq!(
            headers.get("authorization").unwrap(),
            "Basic ZW52LWNsaWVudDplbnYtc2VjcmV0"
        );
    }

    #[test]
    fn plugin_auth_hmac_signs_method_path_body_timestamp_nonce() {
        let provider = CustomHttpProvider::new_with_secret_slots(
            "https://api.example.com",
            HashMap::from([("signing".to_string(), "hmac-secret".to_string())]),
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "hmac",
                        "secret_slot": "signing",
                        "hmac": {
                            "signature_header": "X-Kooix-Signature",
                            "timestamp_header": "X-Kooix-Timestamp",
                            "nonce_header": "X-Kooix-Nonce",
                            "signed_payload": "{{method}}\n{{path}}\n{{query}}\n{{body_sha256}}\n{{timestamp}}\n{{nonce}}",
                            "signature_encoding": "hex"
                        }
                    },
                    "request": {
                        "path": "/private/chat/{{model}}",
                        "query": { "stream": "{{stream}}" },
                        "body": { "prompt": "{{last_user_message}}", "stream": "{{stream}}" }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let req = make_req(false);
        let body = provider.request_json_body(&req).unwrap();
        let endpoint = provider.endpoint_url_for(&req).unwrap();
        let ctx = provider.request_context_for(&req).unwrap();
        let signature = provider
            .hmac_signature(&endpoint, &body, "POST", "1700000000", "nonce-1", &ctx)
            .unwrap();

        assert_eq!(
            provider
                .hmac_signed_payload(&endpoint, &body, "POST", "1700000000", "nonce-1", &ctx)
                .unwrap(),
            format!(
                "POST\n/private/chat/odd-model\nstream=false\n{}\n1700000000\nnonce-1",
                sha256_hex(&body)
            )
        );
        assert_eq!(
            signature,
            "d7304b247aa7c8ddc7618cca688b5f2f1de8dd13cc5169739655a9348510e854"
        );

        let headers = provider
            .request_headers_for_parts(&req, &endpoint, &body, "POST")
            .unwrap();
        assert!(headers.get("x-kooix-timestamp").is_some());
        assert!(headers.get("x-kooix-nonce").is_some());
        assert_eq!(
            headers
                .get("x-kooix-signature")
                .unwrap()
                .to_str()
                .unwrap()
                .len(),
            64
        );
        assert!(headers.get("authorization").is_none());
    }

    #[test]
    fn plugin_auth_aws_sigv4_signs_bedrock_request() {
        let provider = CustomHttpProvider::new_with_secret_slots(
            "https://bedrock-runtime.us-east-1.amazonaws.com",
            HashMap::from([
                ("primary".to_string(), "AKIDEXAMPLE".to_string()),
                (
                    "aws_secret_key".to_string(),
                    "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
                ),
                ("aws_session_token".to_string(), "session-token".to_string()),
            ]),
            json!({
                "plugin": {
                    "version": 1,
                    "preset": { "provider": "bedrock_converse" },
                    "auth": {
                        "strategy": "aws_sigv4",
                        "aws_sigv4": {
                            "service": "bedrock",
                            "region": "us-east-1"
                        }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let req = make_req(false);
        let body = provider.request_json_body(&req).unwrap();
        let endpoint = provider.endpoint_url_for(&req).unwrap();
        let signature = provider
            .aws_sigv4_signature(&endpoint, &body, "POST", "20260519T092000Z", "20260519")
            .unwrap();

        assert!(signature.canonical_request.starts_with(concat!(
            "POST\n",
            "/model/odd-model/converse\n\n",
            "host:bedrock-runtime.us-east-1.amazonaws.com\n",
            "x-amz-content-sha256:"
        )));
        assert!(signature.string_to_sign.starts_with(
            "AWS4-HMAC-SHA256\n20260519T092000Z\n20260519/us-east-1/bedrock/aws4_request\n"
        ));
        assert_eq!(
            signature.authorization,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20260519/us-east-1/bedrock/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=ceffee8ab945dd52eba6a21f6f61d5fd27c7b138ff1c6403c1815c1adebf3f9e"
        );

        let mut headers = HeaderMap::new();
        provider
            .apply_aws_sigv4_auth_headers_at(
                &mut headers,
                &endpoint,
                &body,
                "POST",
                "20260519T092000Z",
                "20260519",
            )
            .unwrap();
        assert!(headers.get("authorization").is_some());
        assert_eq!(
            headers.get("x-amz-date").unwrap().to_str().unwrap(),
            "20260519T092000Z"
        );
        assert_eq!(
            headers
                .get("x-amz-security-token")
                .unwrap()
                .to_str()
                .unwrap(),
            "session-token"
        );
        assert!(headers.get("x-amz-access-key").is_none());
        assert!(headers.get("x-amz-secret-key").is_none());
    }

    #[tokio::test]
    async fn plugin_auth_oauth_client_credentials_caches_until_expiry() {
        let token_server = wiremock::MockServer::start().await;
        let chat_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/oauth/token"))
            .and(wiremock::matchers::body_string_contains(
                "grant_type=client_credentials",
            ))
            .and(wiremock::matchers::body_string_contains(
                "client_id=client-1",
            ))
            .and(wiremock::matchers::body_string_contains(
                "client_secret=secret-1",
            ))
            .and(wiremock::matchers::body_string_contains(
                "scope=chat%3Awrite",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "oauth-token-1",
                "token_type": "Bearer",
                "expires_in": 120
            })))
            .expect(1)
            .mount(&token_server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/private/chat"))
            .and(wiremock::matchers::header(
                "authorization",
                "Bearer oauth-token-1",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-oauth",
                "model": "odd-model",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "ok" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
            })))
            .expect(2)
            .mount(&chat_server)
            .await;

        let provider = CustomHttpProvider::new_with_secret_slots(
            chat_server.uri(),
            HashMap::from([
                ("client_id".to_string(), "client-1".to_string()),
                ("client_secret".to_string(), "secret-1".to_string()),
            ]),
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "oauth_client_credentials",
                        "oauth": {
                            "token_url": format!("{}/oauth/token", token_server.uri()),
                            "scope": "chat:write"
                        }
                    },
                    "security": {
                        "permissions": { "oauth_client_credentials": true }
                    },
                    "request": { "path": "/private/chat" }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let first = provider.chat(make_req(false)).await.unwrap();
        let second = provider.chat(make_req(false)).await.unwrap();
        assert_eq!(first.choices[0].message.content_text(), "ok");
        assert_eq!(second.usage.total_tokens, 2);
    }

    #[tokio::test]
    async fn plugin_embedding_posts_openai_compatible_body_to_embeddings_path() {
        let server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/embeddings"))
            .and(wiremock::matchers::header(
                "authorization",
                "Bearer embed-key",
            ))
            .and(wiremock::matchers::body_json(json!({
                "model": "text-embedding-3-small",
                "input": ["hello", "world"],
                "encoding_format": "float",
                "dimensions": 3
            })))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "object": "list",
                "data": [
                    { "object": "embedding", "index": 0, "embedding": [0.1, 0.2, 0.3] },
                    { "object": "embedding", "index": 1, "embedding": [0.4, 0.5, 0.6] }
                ],
                "model": "text-embedding-3-small",
                "usage": { "prompt_tokens": 4, "total_tokens": 4 }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let provider = CustomHttpProvider::new_with_opts(
            server.uri(),
            "embed-key",
            json!({
                "plugin": {
                    "version": 1,
                    "preset": { "provider": "openai_compatible" }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let response = provider
            .embed(EmbeddingRequest {
                model: "text-embedding-3-small".into(),
                input: EmbeddingInput::Multiple(vec!["hello".into(), "world".into()]),
                encoding_format: Some("float".into()),
                dimensions: Some(3),
            })
            .await
            .unwrap();

        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].embedding, vec![0.1, 0.2, 0.3]);
        assert_eq!(response.usage.total_tokens, 4);
    }

    #[tokio::test]
    async fn plugin_auth_oauth_client_credentials_refreshes_expired_token() {
        let token_server = wiremock::MockServer::start().await;
        let chat_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/oauth/token"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "short-token",
                "expires_in": 1
            })))
            .expect(2)
            .mount(&token_server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/private/chat"))
            .and(wiremock::matchers::header(
                "authorization",
                "Bearer short-token",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-oauth",
                "model": "odd-model",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "ok" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
            })))
            .expect(2)
            .mount(&chat_server)
            .await;

        let provider = CustomHttpProvider::new_with_secret_slots(
            chat_server.uri(),
            HashMap::from([
                ("client_id".to_string(), "client-1".to_string()),
                ("client_secret".to_string(), "secret-1".to_string()),
            ]),
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "oauth_client_credentials",
                        "oauth": {
                            "token_url": format!("{}/oauth/token", token_server.uri()),
                            "expiry_skew_seconds": 0
                        }
                    },
                    "security": {
                        "permissions": { "oauth_client_credentials": true }
                    },
                    "request": { "path": "/private/chat" }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        provider.chat(make_req(false)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        provider.chat(make_req(false)).await.unwrap();
    }

    #[tokio::test]
    async fn plugin_auth_oauth_client_credentials_rejects_invalid_token_response() {
        let token_server = wiremock::MockServer::start().await;
        let chat_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/oauth/token"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "expires_in": 3600
            })))
            .expect(1)
            .mount(&token_server)
            .await;

        let provider = CustomHttpProvider::new_with_secret_slots(
            chat_server.uri(),
            HashMap::from([
                ("client_id".to_string(), "client-1".to_string()),
                ("client_secret".to_string(), "secret-1".to_string()),
            ]),
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "oauth_client_credentials",
                        "oauth": {
                            "token_url": format!("{}/oauth/token", token_server.uri())
                        }
                    },
                    "security": {
                        "permissions": { "oauth_client_credentials": true }
                    },
                    "request": { "path": "/private/chat" }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let err = provider.chat(make_req(false)).await.unwrap_err();
        assert!(err.to_string().contains("access_token"), "err={err}");
    }

    #[test]
    fn preset_allows_request_overrides_without_losing_response_defaults() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://proxy.internal",
            "sk-test",
            json!({
                "plugin": {
                    "preset": { "provider": "openai_compatible" },
                    "request": {
                        "chat_path": "/custom/chat",
                        "headers": { "X-Proxy-Key": "{{api_key}}" }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        assert_eq!(
            provider.endpoint_url_for(&make_req(false)).unwrap(),
            "https://proxy.internal/custom/chat"
        );
        assert!(provider.manifest.response.is_openai_compatible());
        assert_eq!(
            provider
                .request_headers_for(&make_req(false))
                .unwrap()
                .get("x-proxy-key")
                .unwrap(),
            "sk-test"
        );
    }

    #[test]
    fn plugin_manifest_blocks_absolute_chat_path_by_default() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({
                "plugin": {
                    "request": {
                        "chat_path": "http://169.254.169.254/latest/meta-data"
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let err = provider.endpoint_url_for(&make_req(false)).unwrap_err();
        assert!(
            err.to_string().contains("absolute URLs are disabled"),
            "err={err}"
        );
    }

    #[test]
    fn plugin_sandbox_validate_endpoint_blocks_absolute_url_without_permission() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({ "plugin": {} }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let err = provider
            .sandbox
            .validate_endpoint("https://api.other.example/v1", EndpointKind::AbsolutePath)
            .unwrap_err();
        assert!(
            err.to_string().contains("absolute URLs are disabled"),
            "err={err}"
        );
    }

    #[test]
    fn plugin_manifest_rejects_internal_absolute_url_even_when_enabled() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({
                "plugin": {
                    "request": {
                        "chat_path": "http://localhost/admin"
                    },
                    "security": {
                        "allow_absolute_chat_path": true,
                        "permissions": { "absolute_urls": true }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let err = provider.endpoint_url_for(&make_req(false)).unwrap_err();
        assert!(
            err.to_string().contains("forbidden host localhost"),
            "err={err}"
        );
    }

    #[test]
    fn plugin_manifest_requires_permission_for_absolute_urls() {
        let err = match CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({
                "plugin": {
                    "request": {
                        "chat_path": "https://api.other.example/v1/chat"
                    },
                    "security": {
                        "allow_absolute_chat_path": true
                    }
                }
            }),
            crate::ProviderOpts::default(),
        ) {
            Ok(_) => panic!("manifest should require absolute_urls permission"),
            Err(err) => err,
        };

        assert!(
            err.to_string().contains("permissions.absolute_urls"),
            "err={err}"
        );
    }

    #[test]
    fn plugin_manifest_enforces_outbound_allowlist() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({
                "plugin": {
                    "security": {
                        "outbound_allowlist": ["https://api.example.com"]
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let err = provider
            .sandbox
            .validate_endpoint("https://blocked.example/chat", EndpointKind::BaseUrl)
            .unwrap_err();
        assert!(err.to_string().contains("outbound_allowlist"), "err={err}");
    }

    #[test]
    fn plugin_manifest_redacts_headers_and_query_secrets_for_probe_debug() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({
                "plugin": {
                    "auth": { "strategy": "api_key_query", "query_name": "api_key" },
                    "request": {
                        "path": "/chat",
                        "headers": { "X-Trace-Secret": "{{api_key}}" }
                    },
                    "security": {
                        "header_redaction": ["x-trace-secret"]
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let mut endpoint = provider.endpoint_url_for(&make_req(false)).unwrap();
        endpoint.push_str("?api_key=sk-test");
        let redacted_url = provider.sandbox.redact_url(&endpoint);
        assert!(redacted_url.contains("api_key="));
        assert!(!redacted_url.contains("sk-test"), "url={redacted_url}");

        let headers = provider.request_headers_for(&make_req(false)).unwrap();
        let redacted_headers = provider.sandbox.redact_headers(&headers);
        assert_eq!(
            redacted_headers
                .get("x-trace-secret")
                .unwrap()
                .to_str()
                .unwrap(),
            "[REDACTED]"
        );
    }

    #[test]
    fn plugin_dns_rebind_guard_rejects_private_resolved_addresses() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({ "plugin": {} }),
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let err = provider
            .sandbox
            .validate_resolved_addrs(
                "evil.example",
                &[SocketAddr::from(([169, 254, 169, 254], 80))],
            )
            .unwrap_err();

        assert!(err.to_string().contains("DNS rebind guard"), "err={err}");
    }

    #[test]
    fn plugin_manifest_rejects_unknown_header_template_variable() {
        let err = match CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({
                "plugin": {
                    "request": {
                        "headers": { "X-Leak": "{{request.messages}}" }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        ) {
            Ok(_) => panic!("manifest should reject unsupported header template variable"),
            Err(err) => err,
        };

        assert!(
            err.to_string().contains("unsupported template variable"),
            "err={err}"
        );
    }

    #[test]
    fn plugin_request_body_size_limit_is_enforced() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({
                "plugin": {
                    "request": {
                        "body": { "payload": "{{last_user_message}}" }
                    },
                    "security": {
                        "max_request_bytes": 32
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let err = provider.request_json_body(&make_req(false)).unwrap_err();
        assert!(
            err.to_string().contains("plugin request body too large"),
            "err={err}"
        );
    }

    #[tokio::test]
    async fn maps_weird_sse_frames_to_openai_chunks() {
        let manifest = PluginManifest::from_value(
            json!({
                "stream": {
                    "openai_compatible": false,
                    "event_path": "payload",
                    "id_path": "rid",
                    "model_path": "model_name",
                    "role_path": "speaker",
                    "content_path": "token",
                    "finish_reason_path": "reason",
                    "done": ["EOF"],
                    "usage": {
                        "prompt_tokens_path": "usage.in",
                        "completion_tokens_path": "usage.out"
                    }
                }
            }),
            "http://x",
        )
        .unwrap();
        let mapper = StreamMapper {
            stream: manifest.stream,
            fallback_id: "fallback".into(),
            fallback_model: "odd-model".into(),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            max_sse_event_bytes: DEFAULT_MAX_SSE_EVENT_BYTES,
        };
        let sse = concat!(
            "event: token\n",
            "data: {\"payload\":{\"rid\":\"r1\",\"model_name\":\"m1\",\"speaker\":\"assistant\"}}\n\n",
            "data: {\"payload\":{\"token\":\"he\"}}\n\n",
            "data: {\"payload\":{\"token\":\"llo\"}}\n\n",
            "data: {\"payload\":{\"reason\":\"done\",\"usage\":{\"in\":3,\"out\":2}}}\n\n",
            "data: EOF\n\n"
        );
        let stream =
            futures::stream::once(async move { Ok(bytes::Bytes::from_static(sse.as_bytes())) });
        let chunks: Vec<_> = normalize_plugin_sse(stream, mapper)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].choices[0].delta.role, Some(Role::Assistant));
        assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("he"));
        assert_eq!(chunks[2].choices[0].delta.content.as_deref(), Some("llo"));
        assert_eq!(chunks[3].choices[0].finish_reason, Some(FinishReason::Stop));
        assert_eq!(chunks[3].usage.as_ref().unwrap().total_tokens, 5);
    }

    #[test]
    fn replays_manifest_driven_sse_events_tool_calls_usage_and_done_object() {
        let manifest = json!({
            "plugin": {
                "stream": {
                    "openai_compatible": false,
                    "event_path": "payload",
                    "ignore_events": ["ping"],
                    "done_events": ["close"],
                    "done": ["EOF"],
                    "done_path": "type",
                    "done_values": ["message_stop", { "kind": "done" }],
                    "id_path": "rid",
                    "model_path": "model_name",
                    "role_path": "speaker",
                    "content_path": "token",
                    "tool_calls_path": "tool_calls",
                    "finish_reason_path": "finish",
                    "usage": {
                        "prompt_tokens_path": "usage.input",
                        "cached_tokens_path": "usage.cached",
                        "reasoning_tokens_path": "usage.reasoning",
                        "raw_path": "usage"
                    }
                }
            }
        });
        let sse = concat!(
            "event: ping\n",
            "data: {\"payload\":{\"token\":\"ignored\"}}\n\n",
            "event: token\n",
            "data: {\"payload\":{\"rid\":\"r1\",\"model_name\":\"native\",\"speaker\":\"assistant\"}}\n\n",
            "event: token\n",
            "data: {\"payload\":{\"token\":\"he\"}}\n\n",
            "event: tool_delta\n",
            "data: {\"payload\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"q\\\":\"}}]}}\n\n",
            "event: usage\n",
            "data: {\"payload\":{\"usage\":{\"input\":5,\"cached\":2,\"reasoning\":3}}}\n\n",
            "event: token\n",
            "data: {\"payload\":{\"finish\":\"tool_use\"}}\n\n",
            "event: vendor\n",
            "data: {\"payload\":{\"type\":\"message_stop\"}}\n\n",
            "event: close\n",
            "data: {\"payload\":{\"token\":\"ignored-too\"}}\n\n"
        );

        let chunks = replay_plugin_sse(manifest, "http://x", sse, "fallback").unwrap();
        assert_eq!(chunks.len(), 5);
        assert_eq!(chunks[0].id, "r1");
        assert_eq!(chunks[0].model, "native");
        assert_eq!(chunks[0].choices[0].delta.role, Some(Role::Assistant));
        assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("he"));
        assert_eq!(
            chunks[2].choices[0].delta.tool_calls.as_ref().unwrap()[0]
                .function
                .as_ref()
                .unwrap()
                .name
                .as_deref(),
            Some("lookup")
        );
        let usage = chunks[3].usage.as_ref().unwrap();
        assert_eq!(usage.prompt_tokens, 5);
        assert_eq!(usage.cached_tokens, 2);
        assert_eq!(usage.reasoning_tokens, Some(3));
        assert_eq!(usage.raw.as_ref().unwrap()["input"], 5);
        assert_eq!(
            chunks[4].choices[0].finish_reason,
            Some(FinishReason::ToolCalls)
        );
    }

    // ─── ADR-0002 catch_unwind fallback tests ─────────────────────────────

    #[tokio::test]
    async fn run_fastpath_returns_some_for_ok_future() {
        // 用最简单的 manifest 拿一个 CustomHttpProvider 实例（不需要真的发请求）
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
            json!({ "plugin": { "preset": { "provider": "openai" } } }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let result = provider
            .run_fastpath(ProviderPresetKind::Openai, "test_op", async {
                Ok::<u32, ProviderError>(42)
            })
            .await;

        match result {
            Some(Ok(v)) => assert_eq!(v, 42),
            other => panic!("expected Some(Ok(42)), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_fastpath_returns_none_for_panicking_future() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
            json!({ "plugin": { "preset": { "provider": "openai" } } }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        // panic 应该被 catch_unwind 抓住，函数返回 None
        let result = provider
            .run_fastpath::<u32>(ProviderPresetKind::Openai, "test_op", async {
                panic!("simulated fast-path panic");
                #[allow(unreachable_code)]
                Ok(0)
            })
            .await;

        assert!(
            result.is_none(),
            "panicking future must return None to trigger manifest runtime fallback"
        );
    }

    #[test]
    fn panic_message_extracts_string_payload() {
        // 模拟 catch_unwind 返回的 Box<dyn Any + Send>
        let payload: Box<dyn std::any::Any + Send> = Box::new("static str panic");
        assert_eq!(panic_message(&payload), "static str panic");

        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("owned panic"));
        assert_eq!(panic_message(&payload), "owned panic");

        let payload: Box<dyn std::any::Any + Send> = Box::new(42_u32);
        assert_eq!(panic_message(&payload), "<non-string panic>");
    }
}
