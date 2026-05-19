//! Runtime-configurable HTTP provider plugin.
//!
//! A `provider_type` of `plugin` / `custom` / `http` uses channel `model_mapping`
//! as the plugin manifest. The manifest can reshape requests, map strange JSON
//! responses, and normalize arbitrary SSE frames back into OpenAI-compatible chunks.

use crate::Provider;
use crate::error::{
    NormalizedProviderErrorKind, ProviderError, ProviderErrorMetadata, ProviderResult,
};
use crate::openai::check_status;
use crate::plugin_manifest::{
    AuthStrategy, DEFAULT_CHAT_PATH, PluginManifest, ProbeManifest, RequestMethod,
    SignatureEncoding,
};
use crate::plugin_manifest::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_MAX_SSE_EVENT_BYTES};
use crate::plugin_preset::{StreamManifest, adapt_chat_request, eval_path_value};
use crate::sse::{SseEvent, SseLineDecoder};
use crate::types::*;
use async_trait::async_trait;
use base64::Engine as _;
use futures::stream::{BoxStream, StreamExt};
use hmac::{Hmac, Mac};
use reqwest::Method;
use reqwest::Url;
use reqwest::header::{CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

type HmacSha256 = Hmac<Sha256>;

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
        let client = reqwest::Client::builder()
            .connect_timeout(opts.connect_timeout())
            .timeout(opts.timeout_duration())
            .build()
            .map_err(|e| ProviderError::Config(e.to_string()))?;
        Ok(Self {
            client,
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

    fn endpoint_url_for(&self, req: &ChatRequest) -> ProviderResult<String> {
        let ctx = self.request_context_for(req)?;
        self.endpoint_url_with_context(&ctx)
    }

    fn request_method(&self) -> Method {
        match self.manifest.request.method {
            RequestMethod::Get => Method::GET,
            RequestMethod::Post => Method::POST,
        }
    }

    fn endpoint_url_with_context(&self, ctx: &Value) -> ProviderResult<String> {
        let path = self
            .manifest
            .request
            .path
            .as_deref()
            .unwrap_or(DEFAULT_CHAT_PATH);
        let rendered = render_template_str(path, ctx);
        if rendered.starts_with("http://") || rendered.starts_with("https://") {
            if !self.manifest.security.allow_absolute_chat_path {
                return Err(ProviderError::Config(
                    "plugin request.chat_path must be relative; absolute URLs are disabled by default"
                        .into(),
                ));
            }
            validate_http_endpoint(&rendered, true)?;
            return self.url_with_query(rendered, ctx);
        }
        let endpoint = format!("{}{}", self.base_url, slash_path(&rendered));
        validate_http_endpoint(&endpoint, false)?;
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
            validate_http_endpoint(&rendered, true)?;
            return self.url_with_query(rendered, ctx);
        }
        let endpoint = format!("{}{}", self.base_url, slash_path(&rendered));
        validate_http_endpoint(&endpoint, false)?;
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
            .await?;
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
}

#[async_trait]
impl Provider for CustomHttpProvider {
    fn name(&self) -> &'static str {
        "plugin"
    }

    async fn chat(&self, mut req: ChatRequest) -> ProviderResult<ChatResponse> {
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
            .await?;
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
            .await?;
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

#[derive(Clone)]
struct StreamMapper {
    stream: StreamManifest,
    fallback_id: String,
    fallback_model: String,
    max_response_bytes: usize,
    max_sse_event_bytes: usize,
}

fn normalize_plugin_sse<S>(
    byte_stream: S,
    mapper: StreamMapper,
) -> impl futures::Stream<Item = ProviderResult<ChatStreamChunk>>
where
    S: futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    let decoder = SseLineDecoder::new();
    let state = Arc::new(parking_lot::Mutex::new(StreamState {
        id: mapper.fallback_id.clone(),
        model: mapper.fallback_model.clone(),
        response_bytes: 0,
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_tokens: 0,
    }));

    byte_stream.flat_map(move |item| {
        let state = state.clone();
        let mapper = mapper.clone();
        let item = match item {
            Ok(bytes) => {
                {
                    let mut st = state.lock();
                    st.response_bytes = st.response_bytes.saturating_add(bytes.len());
                    if st.response_bytes > mapper.max_response_bytes {
                        return futures::stream::iter(vec![Err(ProviderError::Decode(format!(
                            "plugin response body too large: more than {} bytes",
                            mapper.max_response_bytes
                        )))]);
                    }
                }
                Ok(bytes)
            }
            Err(e) => Err(e),
        };
        let events = match decoder.push(item) {
            Ok(events) => events,
            Err(e) => return futures::stream::iter(vec![Err(e)]),
        };
        let chunks = events
            .into_iter()
            .filter_map(|event| map_plugin_event(event, &mapper, &state))
            .collect::<Vec<_>>();
        futures::stream::iter(chunks)
    })
}

pub fn replay_plugin_sse(
    manifest: Value,
    base_url: &str,
    raw_sse: impl AsRef<[u8]>,
    fallback_model: &str,
) -> ProviderResult<Vec<ChatStreamChunk>> {
    let manifest = PluginManifest::from_value(manifest, base_url)?;
    replay_plugin_sse_with_manifest(manifest.stream, raw_sse, fallback_model)
}

fn replay_plugin_sse_with_manifest(
    stream: StreamManifest,
    raw_sse: impl AsRef<[u8]>,
    fallback_model: &str,
) -> ProviderResult<Vec<ChatStreamChunk>> {
    let mut buf = raw_sse.as_ref().to_vec();
    let events = crate::sse::drain_sse_events(&mut buf);
    if !buf.is_empty() {
        return Err(ProviderError::Decode(
            "raw SSE sample ended with an incomplete event; add a blank line terminator".into(),
        ));
    }
    let mapper = StreamMapper {
        stream,
        fallback_id: format!("chatcmpl-{}", uuid::Uuid::now_v7()),
        fallback_model: fallback_model.to_string(),
        max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        max_sse_event_bytes: DEFAULT_MAX_SSE_EVENT_BYTES,
    };
    let state = parking_lot::Mutex::new(StreamState {
        id: mapper.fallback_id.clone(),
        model: mapper.fallback_model.clone(),
        response_bytes: raw_sse.as_ref().len(),
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_tokens: 0,
    });

    let mut chunks = Vec::new();
    for event in events {
        if let Some(mapped) = map_plugin_event(event, &mapper, &state) {
            chunks.push(mapped?);
        }
    }
    Ok(chunks)
}

#[derive(Debug)]
struct StreamState {
    id: String,
    model: String,
    response_bytes: usize,
    prompt_tokens: u32,
    completion_tokens: u32,
    cached_tokens: u32,
}

fn map_plugin_event(
    event: SseEvent,
    mapper: &StreamMapper,
    state: &parking_lot::Mutex<StreamState>,
) -> Option<ProviderResult<ChatStreamChunk>> {
    if event_name_matches(event.event.as_deref(), &mapper.stream.ignore_events) {
        return None;
    }
    if event_name_matches(event.event.as_deref(), &mapper.stream.done_events) {
        return None;
    }
    let raw = event.data.trim();
    if raw.is_empty() || raw == ":" {
        return None;
    }
    if let Err(e) = enforce_size("plugin SSE event", raw.len(), mapper.max_sse_event_bytes) {
        return Some(Err(e));
    }
    let done_tokens = if mapper.stream.done.is_empty() {
        vec!["[DONE]".to_string()]
    } else {
        mapper.stream.done.clone()
    };
    if done_tokens.iter().any(|d| d == raw) {
        return None;
    }

    if mapper.stream.is_openai_compatible() {
        let value: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => return Some(Err(ProviderError::Decode(format!("sse data {raw:?}: {e}")))),
        };
        return Some(serde_json::from_value(value).map_err(ProviderError::from));
    }

    let value: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => Value::String(raw.to_string()),
    };

    let event_value = mapper
        .stream
        .event_path
        .as_deref()
        .and_then(|p| eval_path_value(&value, p).ok().flatten())
        .unwrap_or_else(|| value.clone());

    let is_vendor_done = vendor_done_object(&event_value, &mapper.stream);

    let mut st = state.lock();
    if let Some(id) = mapper
        .stream
        .id_path
        .as_deref()
        .and_then(|p| eval_path_value(&event_value, p).ok().flatten())
        .as_ref()
        .and_then(value_to_string)
    {
        st.id = id;
    }
    if let Some(model) = mapper
        .stream
        .model_path
        .as_deref()
        .and_then(|p| eval_path_value(&event_value, p).ok().flatten())
        .as_ref()
        .and_then(value_to_string)
    {
        st.model = model;
    }
    let content = mapper
        .stream
        .content_path
        .as_deref()
        .and_then(|p| eval_path_value(&event_value, p).ok().flatten())
        .as_ref()
        .and_then(value_to_string);
    let role = mapper
        .stream
        .role_path
        .as_deref()
        .and_then(|p| eval_path_value(&event_value, p).ok().flatten())
        .as_ref()
        .and_then(value_to_string)
        .and_then(|s| map_role(&s));
    let tool_calls = match mapper
        .stream
        .tool_calls_path
        .as_deref()
        .and_then(|p| eval_path_value(&event_value, p).ok().flatten())
    {
        Some(value) => match serde_json::from_value::<Vec<ToolCallDelta>>(value) {
            Ok(tool_calls) => Some(tool_calls),
            Err(e) => {
                return Some(Err(ProviderError::Decode(format!(
                    "plugin stream tool_calls_path is not a valid tool call delta array: {e}"
                ))));
            }
        },
        None => None,
    };
    let finish_reason = mapper
        .stream
        .finish_reason_path
        .as_deref()
        .and_then(|p| eval_path_value(&event_value, p).ok().flatten())
        .as_ref()
        .and_then(value_to_string)
        .and_then(|s| map_finish_reason(&s));
    let usage = match mapper.stream.usage.extract_optional(&event_value) {
        Ok(Some(usage)) => {
            let emit = mapper
                .stream
                .usage
                .should_emit_stream_usage(&usage, finish_reason);
            let merged = merge_usage_state(usage.usage, &mut st);
            emit.then_some(merged)
        }
        Ok(None) => None,
        Err(e) => return Some(Err(e)),
    };

    if is_vendor_done
        && role.is_none()
        && content.is_none()
        && tool_calls.is_none()
        && finish_reason.is_none()
        && usage.is_none()
    {
        return None;
    }

    if role.is_none()
        && content.is_none()
        && tool_calls.is_none()
        && finish_reason.is_none()
        && usage.is_none()
    {
        return None;
    }

    Some(Ok(ChatStreamChunk {
        id: st.id.clone(),
        model: st.model.clone(),
        choices: vec![ChatStreamChoice {
            index: 0,
            delta: ChatDelta {
                role,
                content,
                tool_calls,
            },
            finish_reason,
        }],
        usage,
    }))
}

fn event_name_matches(event: Option<&str>, patterns: &[String]) -> bool {
    let Some(event) = event.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    patterns.iter().any(|p| p.trim() == event)
}

fn vendor_done_object(event_value: &Value, stream: &StreamManifest) -> bool {
    let Some(path) = stream.done_path.as_deref() else {
        return false;
    };
    let Some(value) = eval_path_value(event_value, path).ok().flatten() else {
        return false;
    };
    stream
        .done_values
        .iter()
        .any(|done| json_values_equal(&value, done))
}

fn json_values_equal(left: &Value, right: &Value) -> bool {
    if left == right {
        return true;
    }
    match (left, right) {
        (Value::String(a), Value::String(b)) => a == b,
        (Value::String(a), other) => value_to_string(other).is_some_and(|b| a == &b),
        (other, Value::String(b)) => value_to_string(other).is_some_and(|a| &a == b),
        _ => false,
    }
}

fn merge_usage_state(usage: Usage, state: &mut StreamState) -> Usage {
    if usage.prompt_tokens > 0 {
        state.prompt_tokens = usage.prompt_tokens;
    }
    if usage.completion_tokens > 0 {
        state.completion_tokens = usage.completion_tokens;
    }
    if usage.cached_tokens > 0 {
        state.cached_tokens = usage.cached_tokens;
    }

    let prompt_tokens = if usage.prompt_tokens > 0 {
        usage.prompt_tokens
    } else {
        state.prompt_tokens
    };
    let completion_tokens = if usage.completion_tokens > 0 {
        usage.completion_tokens
    } else {
        state.completion_tokens
    };
    let cached_tokens = if usage.cached_tokens > 0 {
        usage.cached_tokens
    } else {
        state.cached_tokens
    };
    let inferred_total = prompt_tokens + completion_tokens;
    let total_tokens = usage.total_tokens.max(inferred_total);

    Usage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
        cached_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        image_units: usage.image_units,
        audio_seconds: usage.audio_seconds,
        raw: usage.raw,
    }
}

fn merge_reasoning_content(content: String, reasoning: Option<String>) -> String {
    match (reasoning, content) {
        (Some(reasoning), content) if !reasoning.is_empty() && !content.is_empty() => {
            format!("{reasoning}\n{content}")
        }
        (Some(reasoning), _) if !reasoning.is_empty() => reasoning,
        (_, content) => content,
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

fn enforce_size(name: &str, actual: usize, limit: usize) -> ProviderResult<()> {
    if actual > limit {
        return Err(ProviderError::Decode(format!(
            "{name} too large: {actual} bytes > {limit} bytes"
        )));
    }
    Ok(())
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

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> ProviderResult<Vec<u8>> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| ProviderError::Config(format!("invalid hmac key: {e}")))?;
    mac.update(msg);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn hmac_sha256_hex(key: &[u8], msg: &[u8]) -> ProviderResult<String> {
    hmac_sha256(key, msg).map(hex::encode)
}

fn aws_sigv4_signing_key(
    secret_key: &str,
    date: &str,
    region: &str,
    service: &str,
) -> ProviderResult<Vec<u8>> {
    let k_date = hmac_sha256(format!("AWS4{secret_key}").as_bytes(), date.as_bytes())?;
    let k_region = hmac_sha256(&k_date, region.as_bytes())?;
    let k_service = hmac_sha256(&k_region, service.as_bytes())?;
    hmac_sha256(&k_service, b"aws4_request")
}

fn infer_aws_region_from_host(host: &str) -> Option<String> {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() >= 4
        && labels[0].starts_with("bedrock-runtime")
        && labels.last().is_some_and(|tld| *tld == "com")
    {
        return Some(labels[1].to_string());
    }
    None
}

fn canonical_uri(url: &Url) -> String {
    let path = url.path();
    if path.is_empty() {
        "/".to_string()
    } else {
        path.split('/')
            .map(uri_encode)
            .collect::<Vec<_>>()
            .join("/")
    }
}

fn canonical_query_string(url: &Url) -> String {
    let Some(query) = url.query() else {
        return String::new();
    };
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (uri_encode(&k), uri_encode(&v)))
        .collect();
    pairs.sort();
    if pairs.is_empty() && !query.is_empty() {
        return query.to_string();
    }
    pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn uri_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn normalize_secret_slots(secrets: HashMap<String, String>) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = secrets
        .into_iter()
        .map(|(slot, value)| (normalize_secret_slot(&slot), value))
        .collect();
    if !out.contains_key("primary")
        && let Some(value) = out.get("api_key").cloned()
    {
        out.insert("primary".to_string(), value);
    }
    out
}

fn normalize_secret_slot(slot: &str) -> String {
    let trimmed = slot.trim();
    if trimmed.is_empty() || trimmed == "api_key" {
        "primary".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    }
}

fn env_key_for_secret_slot(slot: &str) -> String {
    let normalized = normalize_secret_slot(slot);
    match normalized.as_str() {
        "primary" => "KOOIX_PLUGIN_SECRET_PRIMARY".to_string(),
        "aws_secret_key" => "AWS_SECRET_ACCESS_KEY".to_string(),
        "aws_session_token" => "AWS_SESSION_TOKEN".to_string(),
        other => format!(
            "KOOIX_PLUGIN_SECRET_{}",
            other
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_uppercase()
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        ),
    }
}

fn env_secret_slots(channel_code: &str) -> HashMap<String, String> {
    let mut secrets = HashMap::new();
    if let Some(primary) = env_primary_secret(channel_code) {
        secrets.insert("primary".to_string(), primary);
    }
    for (slot, env_key) in [
        ("aws_secret_key", "AWS_SECRET_ACCESS_KEY"),
        ("aws_session_token", "AWS_SESSION_TOKEN"),
    ] {
        if let Ok(value) = std::env::var(env_key) {
            secrets.entry(slot.to_string()).or_insert(value);
        }
    }
    for (key, value) in std::env::vars() {
        let Some(slot) = key.strip_prefix("KOOIX_PLUGIN_SECRET_") else {
            continue;
        };
        if slot.is_empty() {
            continue;
        }
        let slot = normalize_secret_slot(slot);
        secrets.entry(slot).or_insert(value);
    }
    secrets
}

fn env_primary_secret(channel_code: &str) -> Option<String> {
    let env_key = format!(
        "KOOIX_CH_{}_KEY",
        channel_code
            .to_uppercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    std::env::var(&env_key)
        .or_else(|_| std::env::var("KOOIX_API_KEY"))
        .or_else(|_| std::env::var("KOOIX_PLUGIN_SECRET_PRIMARY"))
        .ok()
}

fn validate_http_endpoint(endpoint: &str, deny_internal_host: bool) -> ProviderResult<()> {
    let parsed = reqwest::Url::parse(endpoint)
        .map_err(|e| ProviderError::Config(format!("invalid plugin endpoint URL: {e}")))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(ProviderError::Config(format!(
                "plugin endpoint scheme must be http/https, got {other}"
            )));
        }
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| ProviderError::Config("plugin endpoint URL missing host".into()))?;
    if deny_internal_host && is_internal_or_metadata_host(host) {
        return Err(ProviderError::Config(format!(
            "plugin absolute chat_path targets forbidden host {host}"
        )));
    }
    Ok(())
}

fn is_internal_or_metadata_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if matches!(
        host.as_str(),
        "localhost" | "metadata" | "metadata.google.internal"
    ) {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(ip) => {
                ip.is_private()
                    || ip.is_loopback()
                    || ip.is_link_local()
                    || ip.is_unspecified()
                    || ip.is_broadcast()
            }
            IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
            }
        };
    }
    false
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

fn value_to_string(v: &Value) -> Option<String> {
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

fn map_role(s: &str) -> Option<Role> {
    match s {
        "system" => Some(Role::System),
        "user" => Some(Role::User),
        "assistant" | "bot" | "model" => Some(Role::Assistant),
        "tool" => Some(Role::Tool),
        _ => None,
    }
}

fn map_finish_reason(s: &str) -> Option<FinishReason> {
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
                        "allow_absolute_chat_path": true
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
}
