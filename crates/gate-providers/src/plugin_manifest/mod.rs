//! Strongly typed HTTP plugin manifest.
//!
//! Runtime accepts the legacy v0 shape (`{ "plugin": { "preset": ... } }`) and
//! upgrades it into the v1 internal representation. New manifests should use
//! `plugin.version = 1` with fixed sections so backend validation, web forms and
//! CLI lint share one contract.

mod upgrade;
mod validate;

use crate::error::{ProviderError, ProviderResult};
use crate::plugin_preset::{
    EmbeddingResponseManifest, PresetManifest, ProviderPresetSpec, ResponseManifest,
    StreamManifest, UsageManifest,
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use upgrade::{deserialize_v1, extract_plugin_value, upgrade_v0};
use validate::{
    TemplateScope, header_belongs_to_auth, required_secret_slots, secret_slot_declared,
    validate_auth, validate_embedding_response_paths, validate_header_name, validate_limit,
    validate_metadata_urls, validate_outbound_allowlist_entry, validate_probe,
    validate_response_paths, validate_secret_slot, validate_stream_paths, validate_template_str,
    validate_template_value, validate_timeout_ms,
};

pub(crate) const DEFAULT_CHAT_PATH: &str = "/chat/completions";
pub(crate) const DEFAULT_EMBEDDINGS_PATH: &str = "/embeddings";
pub(crate) const DEFAULT_MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub(crate) const DEFAULT_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const DEFAULT_MAX_SSE_EVENT_BYTES: usize = 1024 * 1024;
const HARD_MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
const HARD_MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const HARD_MAX_SSE_EVENT_BYTES: usize = 4 * 1024 * 1024;
const HARD_MAX_TIMEOUT_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub struct ChannelPluginMapping {
    pub plugin: PluginManifest,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct PluginManifest {
    pub version: u8,
    pub metadata: MetadataManifest,
    pub capabilities: CapabilitiesManifest,
    pub auth: AuthManifest,
    pub request: RequestManifest,
    pub(crate) embedding_response: EmbeddingResponseManifest,
    pub(crate) response: ResponseManifest,
    pub(crate) stream: StreamManifest,
    pub(crate) usage: UsageManifest,
    pub error: ErrorManifest,
    pub probe: ProbeManifest,
    pub security: SecurityManifest,
    pub(crate) preset: PresetManifest,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub struct MetadataManifest {
    pub name: Option<String>,
    pub vendor: Option<String>,
    pub homepage: Option<String>,
    pub docs: Option<String>,
    pub owner: Option<String>,
    pub tags: Vec<String>,
}

pub type CapabilitiesManifest = crate::capabilities::ProviderCapabilities;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthStrategy {
    #[default]
    Bearer,
    ApiKeyHeader,
    ApiKeyQuery,
    Basic,
    CustomHeaders,
    Hmac,
    AwsSigv4,
    OauthClientCredentials,
    None,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct AuthManifest {
    pub strategy: AuthStrategy,
    pub secret_slot: Option<String>,
    pub header_name: Option<String>,
    pub query_name: Option<String>,
    pub username_slot: Option<String>,
    pub password_slot: Option<String>,
    pub headers: Map<String, Value>,
    pub hmac: HmacAuthManifest,
    pub aws_sigv4: AwsSigv4AuthManifest,
    pub oauth: OauthClientCredentialsManifest,
}

impl AuthManifest {
    pub(crate) fn secret_slot(&self) -> &str {
        self.secret_slot.as_deref().unwrap_or("primary")
    }

    pub(crate) fn header_name(&self) -> Option<&str> {
        self.header_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    pub(crate) fn query_name(&self) -> Option<&str> {
        self.query_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    pub(crate) fn username_slot(&self) -> Option<&str> {
        self.username_slot
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    pub(crate) fn password_slot(&self) -> Option<&str> {
        self.password_slot
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
}

impl ProbeManifest {
    pub fn success_status_or_default(&self) -> Vec<u16> {
        if self.success_status.is_empty() {
            vec![200]
        } else {
            self.success_status.clone()
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HmacAlgorithm {
    #[default]
    Sha256,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct HmacAuthManifest {
    pub algorithm: HmacAlgorithm,
    pub signature_header: String,
    pub timestamp_header: String,
    pub nonce_header: String,
    pub signed_payload: String,
    pub signature_encoding: SignatureEncoding,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SignatureEncoding {
    Base64,
    #[default]
    Hex,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct AwsSigv4AuthManifest {
    pub service: String,
    pub region: Option<String>,
    pub access_key_slot: String,
    pub secret_key_slot: String,
    pub session_token_slot: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct OauthClientCredentialsManifest {
    pub token_url: String,
    pub client_id_slot: String,
    pub client_secret_slot: String,
    pub scope: Option<String>,
    pub audience: Option<String>,
    pub token_type: String,
    pub expiry_skew_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum RequestMethod {
    Get,
    #[default]
    Post,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct RequestManifest {
    pub method: RequestMethod,
    #[serde(alias = "chat_path")]
    pub path: Option<String>,
    #[serde(alias = "embeddings_path")]
    pub embedding_path: Option<String>,
    pub query: Map<String, Value>,
    pub headers: Map<String, Value>,
    pub body: Option<Value>,
    pub embedding_body: Option<Value>,
    pub timeout_ms: Option<u64>,
    pub retry: RetryManifest,
    pub force_stream_field: bool,
    pub stream_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub struct RetryManifest {
    pub max_retries: Option<u8>,
    pub retryable_status: Vec<u16>,
    pub retryable_codes: Vec<String>,
    pub cooldown_ms: Option<u64>,
    pub circuit_breaker_failures: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub struct ErrorManifest {
    pub status_path: Option<String>,
    pub code_path: Option<String>,
    pub message_path: Option<String>,
    pub retryable_status: Vec<u16>,
    pub retryable_codes: Vec<String>,
    pub auth_status: Vec<u16>,
    pub rate_limit_status: Vec<u16>,
    pub model_not_found_status: Vec<u16>,
    pub safety_block_codes: Vec<String>,
    pub cooldown_ms: Option<u64>,
    pub circuit_breaker_failures: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub struct ProbeManifest {
    pub model: Option<String>,
    pub path: Option<String>,
    pub body: Option<Value>,
    pub success_status: Vec<u16>,
    pub max_cost_micros: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub struct SecurityManifest {
    pub outbound_allowlist: Vec<String>,
    pub header_redaction: Vec<String>,
    pub permissions: PluginPermissionsManifest,
    pub max_request_bytes: Option<usize>,
    pub max_response_bytes: Option<usize>,
    pub max_sse_event_bytes: Option<usize>,
    pub allow_absolute_chat_path: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct PluginPermissionsManifest {
    pub outbound_http: bool,
    pub absolute_urls: bool,
    pub oauth_client_credentials: bool,
    pub secret_slots: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct LegacyPluginManifest {
    preset: PresetManifest,
    request: LegacyRequestManifest,
    response: ResponseManifest,
    embedding_response: EmbeddingResponseManifest,
    stream: StreamManifest,
    error: ErrorManifest,
    probe: ProbeManifest,
    security: SecurityManifest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct LegacyRequestManifest {
    chat_path: Option<String>,
    embedding_path: Option<String>,
    embeddings_path: Option<String>,
    headers: Map<String, Value>,
    body: Option<Value>,
    embedding_body: Option<Value>,
    retry: RetryManifest,
    force_stream_field: bool,
    stream_path: String,
}

impl PluginManifest {
    pub(crate) fn from_value(value: Value, base_url: &str) -> ProviderResult<Self> {
        let (manifest_value, pointer_base) = extract_plugin_value(value);
        let mut manifest = if manifest_value.is_null() || manifest_value == json!({}) {
            Self::default()
        } else if manifest_value.get("version").and_then(Value::as_u64) == Some(1) {
            deserialize_v1(manifest_value, &pointer_base)?
        } else {
            upgrade_v0(manifest_value, &pointer_base)?
        };

        manifest.apply_preset(base_url)?;
        manifest.validate(&pointer_base)?;
        Ok(manifest)
    }

    fn apply_preset(&mut self, base_url: &str) -> ProviderResult<()> {
        let Some(kind) = self.preset.kind else {
            return Ok(());
        };
        let spec =
            ProviderPresetSpec::for_kind(kind, base_url, self.preset.api_version.as_deref())?;
        self.preset.adapter = spec.adapter;
        self.capabilities.merge_truthy_defaults(&spec.capabilities);
        match kind {
            crate::plugin_preset::ProviderPresetKind::AzureOpenai
                if self.auth.strategy == AuthStrategy::Bearer =>
            {
                self.auth.strategy = AuthStrategy::ApiKeyHeader;
                self.auth
                    .header_name
                    .get_or_insert_with(|| "api-key".to_string());
            }
            crate::plugin_preset::ProviderPresetKind::AnthropicMessages
                if self.auth.strategy == AuthStrategy::Bearer =>
            {
                self.auth.strategy = AuthStrategy::ApiKeyHeader;
                self.auth
                    .header_name
                    .get_or_insert_with(|| "x-api-key".to_string());
            }
            crate::plugin_preset::ProviderPresetKind::BedrockConverse
                if self.auth.strategy == AuthStrategy::Bearer =>
            {
                self.auth.strategy = AuthStrategy::AwsSigv4;
            }
            _ => {}
        }
        if self.request.path.is_none() {
            self.request.path = Some(spec.chat_path);
        }
        if self.request.embedding_path.is_none() {
            self.request.embedding_path = spec.embedding_path;
        }
        if self.request.body.is_none() {
            self.request.body = spec.body;
        }
        if self.request.embedding_body.is_none() {
            self.request.embedding_body = spec.embedding_body;
        }
        for (k, v) in spec.headers {
            if k.eq_ignore_ascii_case("authorization") && v.is_null() {
                continue;
            }
            let is_runtime_auth_header = matches!(
                self.auth.strategy,
                AuthStrategy::ApiKeyHeader | AuthStrategy::CustomHeaders
            ) && header_belongs_to_auth(&self.auth, &k);
            if !is_runtime_auth_header {
                self.request.headers.entry(k).or_insert(v);
            }
        }
        if let Some(path) = spec.stream_path {
            self.request.stream_path = path;
        }
        self.response.apply_defaults(spec.response);
        self.embedding_response
            .apply_defaults(spec.embedding_response);
        self.stream.apply_defaults(spec.stream);
        Ok(())
    }

    fn validate(&self, pointer_base: &str) -> ProviderResult<()> {
        if self.version != 1 {
            return Err(config_at(
                pointer_base,
                "/version",
                "plugin.version must be 1",
            ));
        }

        validate_metadata_urls(&self.metadata, pointer_base)?;
        validate_auth(&self.auth, pointer_base)?;

        if let Some(path) = &self.request.path {
            validate_template_str(
                path,
                TemplateScope::Path,
                &json_pointer(pointer_base, "/request/path"),
            )?;
        }
        if let Some(path) = &self.request.embedding_path {
            validate_template_str(
                path,
                TemplateScope::Path,
                &json_pointer(pointer_base, "/request/embedding_path"),
            )?;
        }
        for (name, value) in &self.request.query {
            validate_template_value(
                value,
                TemplateScope::Query,
                &json_pointer(pointer_base, &format!("/request/query/{name}")),
            )?;
        }
        for (name, value) in &self.request.headers {
            validate_template_value(
                value,
                TemplateScope::Header,
                &json_pointer(pointer_base, &format!("/request/headers/{name}")),
            )?;
        }
        if let Some(body) = &self.request.body {
            validate_template_value(
                body,
                TemplateScope::Body,
                &json_pointer(pointer_base, "/request/body"),
            )?;
        }
        if let Some(body) = &self.request.embedding_body {
            validate_template_value(
                body,
                TemplateScope::Body,
                &json_pointer(pointer_base, "/request/embedding_body"),
            )?;
        }
        validate_timeout_ms(self.request.timeout_ms, pointer_base)?;
        validate_probe(&self.probe, &json_pointer(pointer_base, "/probe"))?;
        validate_response_paths(&self.response, &json_pointer(pointer_base, "/response"))?;
        validate_embedding_response_paths(
            &self.embedding_response,
            &json_pointer(pointer_base, "/embedding_response"),
        )?;
        validate_stream_paths(&self.stream, &json_pointer(pointer_base, "/stream"))?;
        self.security.validate(pointer_base)?;
        self.validate_sandbox_permissions(pointer_base)
    }

    fn validate_sandbox_permissions(&self, pointer_base: &str) -> ProviderResult<()> {
        let permissions = &self.security.permissions;
        if !permissions.outbound_http {
            return Err(config_at(
                pointer_base,
                "/security/permissions/outbound_http",
                "HTTP plugin runtime requires outbound_http permission",
            ));
        }
        if self.security.allow_absolute_chat_path && !permissions.absolute_urls {
            return Err(config_at(
                pointer_base,
                "/security/permissions/absolute_urls",
                "allow_absolute_chat_path requires permissions.absolute_urls=true",
            ));
        }
        if self.auth.strategy == AuthStrategy::OauthClientCredentials
            && !permissions.oauth_client_credentials
        {
            return Err(config_at(
                pointer_base,
                "/security/permissions/oauth_client_credentials",
                "oauth_client_credentials auth requires permissions.oauth_client_credentials=true",
            ));
        }

        if !permissions.secret_slots.is_empty() {
            for slot in required_secret_slots(&self.auth) {
                if !secret_slot_declared(&permissions.secret_slots, &slot) {
                    return Err(config_at(
                        pointer_base,
                        "/security/permissions/secret_slots",
                        &format!("secret slot {slot:?} is used but not declared"),
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Default for PluginManifest {
    fn default() -> Self {
        Self {
            version: 1,
            metadata: MetadataManifest::default(),
            capabilities: CapabilitiesManifest {
                chat: true,
                streaming: true,
                ..Default::default()
            },
            auth: AuthManifest::default(),
            request: RequestManifest::default(),
            embedding_response: EmbeddingResponseManifest::default(),
            response: ResponseManifest::default(),
            stream: StreamManifest::default(),
            usage: UsageManifest::default(),
            error: ErrorManifest::default(),
            probe: ProbeManifest::default(),
            security: SecurityManifest::default(),
            preset: PresetManifest::default(),
        }
    }
}

impl Default for AuthManifest {
    fn default() -> Self {
        Self {
            strategy: AuthStrategy::Bearer,
            secret_slot: Some("primary".to_string()),
            header_name: None,
            query_name: None,
            username_slot: None,
            password_slot: None,
            headers: Map::new(),
            hmac: HmacAuthManifest::default(),
            aws_sigv4: AwsSigv4AuthManifest::default(),
            oauth: OauthClientCredentialsManifest::default(),
        }
    }
}

impl Default for HmacAuthManifest {
    fn default() -> Self {
        Self {
            algorithm: HmacAlgorithm::Sha256,
            signature_header: "X-Signature".to_string(),
            timestamp_header: "X-Timestamp".to_string(),
            nonce_header: "X-Nonce".to_string(),
            signed_payload: "{{method}}\n{{path}}\n{{body_sha256}}\n{{timestamp}}\n{{nonce}}"
                .to_string(),
            signature_encoding: SignatureEncoding::Hex,
        }
    }
}

impl Default for AwsSigv4AuthManifest {
    fn default() -> Self {
        Self {
            service: "bedrock".to_string(),
            region: None,
            access_key_slot: "primary".to_string(),
            secret_key_slot: "aws_secret_key".to_string(),
            session_token_slot: Some("aws_session_token".to_string()),
        }
    }
}

impl Default for OauthClientCredentialsManifest {
    fn default() -> Self {
        Self {
            token_url: String::new(),
            client_id_slot: "client_id".to_string(),
            client_secret_slot: "client_secret".to_string(),
            scope: None,
            audience: None,
            token_type: "Bearer".to_string(),
            expiry_skew_seconds: 60,
        }
    }
}

impl Default for RequestManifest {
    fn default() -> Self {
        Self {
            method: RequestMethod::Post,
            path: None,
            embedding_path: None,
            query: Map::new(),
            headers: Map::new(),
            body: None,
            embedding_body: None,
            retry: RetryManifest::default(),
            timeout_ms: None,
            force_stream_field: true,
            stream_path: "stream".to_string(),
        }
    }
}

impl SecurityManifest {
    pub(crate) fn max_request_bytes(&self) -> usize {
        self.max_request_bytes.unwrap_or(DEFAULT_MAX_REQUEST_BYTES)
    }

    pub(crate) fn max_response_bytes(&self) -> usize {
        self.max_response_bytes
            .unwrap_or(DEFAULT_MAX_RESPONSE_BYTES)
    }

    pub(crate) fn max_sse_event_bytes(&self) -> usize {
        self.max_sse_event_bytes
            .unwrap_or(DEFAULT_MAX_SSE_EVENT_BYTES)
    }

    fn validate(&self, pointer_base: &str) -> ProviderResult<()> {
        for (idx, entry) in self.outbound_allowlist.iter().enumerate() {
            validate_outbound_allowlist_entry(
                entry,
                &json_pointer(pointer_base, &format!("/security/outbound_allowlist/{idx}")),
            )?;
        }
        for (idx, name) in self.header_redaction.iter().enumerate() {
            validate_header_name(
                name,
                &json_pointer(pointer_base, &format!("/security/header_redaction/{idx}")),
            )?;
        }
        self.permissions.validate(pointer_base)?;
        validate_limit(
            &json_pointer(pointer_base, "/security/max_request_bytes"),
            self.max_request_bytes(),
            HARD_MAX_REQUEST_BYTES,
        )?;
        validate_limit(
            &json_pointer(pointer_base, "/security/max_response_bytes"),
            self.max_response_bytes(),
            HARD_MAX_RESPONSE_BYTES,
        )?;
        validate_limit(
            &json_pointer(pointer_base, "/security/max_sse_event_bytes"),
            self.max_sse_event_bytes(),
            HARD_MAX_SSE_EVENT_BYTES,
        )
    }
}

impl Default for PluginPermissionsManifest {
    fn default() -> Self {
        Self {
            outbound_http: true,
            absolute_urls: false,
            oauth_client_credentials: false,
            secret_slots: Vec::new(),
        }
    }
}

impl PluginPermissionsManifest {
    fn validate(&self, pointer_base: &str) -> ProviderResult<()> {
        for (idx, slot) in self.secret_slots.iter().enumerate() {
            validate_secret_slot(
                pointer_base,
                &format!("/security/permissions/secret_slots/{idx}"),
                slot,
            )?;
        }
        Ok(())
    }
}

pub fn validate_plugin_manifest(value: Value, base_url: &str) -> ProviderResult<()> {
    PluginManifest::from_value(value, base_url).map(|_| ())
}

pub fn plugin_manifest(value: Value, base_url: &str) -> ProviderResult<PluginManifest> {
    PluginManifest::from_value(value, base_url)
}

pub fn plugin_manifest_retry_config(
    value: &Value,
    base_url: &str,
) -> ProviderResult<crate::retry::RetryConfig> {
    let manifest = PluginManifest::from_value(value.clone(), base_url)?;
    let mut config = crate::retry::RetryConfig::default();
    if let Some(max_retries) = manifest.request.retry.max_retries {
        config.max_retries = max_retries as u32;
    }
    for status in manifest
        .request
        .retry
        .retryable_status
        .iter()
        .chain(manifest.error.retryable_status.iter())
    {
        if !config.retryable_status_codes.contains(status) {
            config.retryable_status_codes.push(*status);
        }
    }
    for code in manifest
        .request
        .retry
        .retryable_codes
        .iter()
        .chain(manifest.error.retryable_codes.iter())
    {
        if !config
            .retryable_error_codes
            .iter()
            .any(|existing| existing == code)
        {
            config.retryable_error_codes.push(code.clone());
        }
    }
    if let Some(cooldown_ms) = manifest
        .request
        .retry
        .cooldown_ms
        .or(manifest.error.cooldown_ms)
    {
        config.max_backoff_ms = config.max_backoff_ms.max(cooldown_ms);
    }
    Ok(config)
}

pub fn plugin_manifest_schema_json() -> Value {
    let mut schema = schema_for!(ChannelPluginMapping).to_value();
    if let Some(obj) = schema.as_object_mut() {
        obj.insert(
            "$id".to_string(),
            json!("https://kooix-gate.local/schemas/plugin-manifest-v1.json"),
        );
        obj.insert(
            "title".to_string(),
            json!("Kooix Gate HTTP Plugin Manifest v1"),
        );
        obj.insert(
            "description".to_string(),
            json!("Schema for channels.model_mapping.plugin. Runtime also accepts legacy v0 manifests and upgrades them to plugin.version=1."),
        );
    }
    schema
}

fn path_to_json_pointer(path: &serde_path_to_error::Path) -> String {
    let mut out = String::new();
    for segment in path {
        match segment {
            serde_path_to_error::Segment::Seq { index } => {
                out.push('/');
                out.push_str(&index.to_string());
            }
            serde_path_to_error::Segment::Map { key }
            | serde_path_to_error::Segment::Enum { variant: key } => {
                out.push('/');
                out.push_str(&escape_json_pointer(key));
            }
            serde_path_to_error::Segment::Unknown => out.push_str("/?"),
        }
    }
    out
}

fn json_pointer(base: &str, suffix: &str) -> String {
    match (base.is_empty(), suffix.is_empty()) {
        (true, true) => "/".to_string(),
        (true, false) => suffix.to_string(),
        (false, true) => base.to_string(),
        (false, false) => format!("{base}{suffix}"),
    }
}

fn escape_json_pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

fn config_at(base: &str, suffix: &str, message: &str) -> ProviderError {
    ProviderError::Config(format!(
        "invalid plugin manifest at {}: {message}",
        json_pointer(base, suffix)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_v1_manifest_and_keeps_fixed_sections() {
        let manifest = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "metadata": { "name": "odd", "vendor": "acme", "tags": ["private"] },
                    "capabilities": { "chat": true, "streaming": true, "tools": true },
                    "auth": { "strategy": "api_key_header", "header_name": "X-Api-Key" },
                    "request": {
                        "method": "POST",
                        "path": "/v1/messages/{{model}}",
                        "query": { "stream": "{{stream}}" },
                        "headers": { "X-Model": "{{model}}" },
                        "body": { "messages": "{{messages}}" }
                    },
                    "embedding_response": {
                        "openai_compatible": false,
                        "data_path": "embeddings.float",
                        "embedding_path": ".",
                        "model_path": "model",
                        "usage": { "prompt_tokens_path": "usage.input", "total_tokens_path": "usage.total" }
                    },
                    "response": { "openai_compatible": false, "content_path": "answer" },
                    "stream": { "openai_compatible": false, "content_path": "token" },
                    "usage": { "prompt_tokens_path": "usage.prompt" },
                    "error": { "message_path": "error.message" },
                    "probe": { "model": "tiny", "success_status": [200] },
                    "security": {
                        "max_request_bytes": 4096,
                        "header_redaction": ["authorization"],
                        "outbound_allowlist": ["https://upstream.example"],
                        "permissions": {
                            "secret_slots": ["primary"]
                        }
                    }
                }
            }),
            "https://upstream.example",
        )
        .unwrap();

        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.metadata.name.as_deref(), Some("odd"));
        assert!(manifest.capabilities.tools);
        assert_eq!(
            manifest.request.path.as_deref(),
            Some("/v1/messages/{{model}}")
        );
        assert_eq!(
            manifest.embedding_response.data_path.as_deref(),
            Some("embeddings.float")
        );
        assert_eq!(manifest.auth.strategy, AuthStrategy::ApiKeyHeader);
        assert_eq!(
            manifest.security.outbound_allowlist,
            vec!["https://upstream.example".to_string()]
        );
        assert!(manifest.security.permissions.outbound_http);
        assert!(!manifest.security.permissions.absolute_urls);
    }

    #[test]
    fn sandbox_permissions_require_explicit_oauth_and_secret_slots() {
        let err = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "oauth_client_credentials",
                        "oauth": {
                            "token_url": "https://idp.example.com/oauth/token"
                        }
                    }
                }
            }),
            "https://upstream.example",
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("permissions.oauth_client_credentials"),
            "err={err}"
        );

        let err = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "api_key_header",
                        "header_name": "X-Api-Key",
                        "secret_slot": "private_key"
                    },
                    "security": {
                        "permissions": {
                            "secret_slots": ["primary"]
                        }
                    }
                }
            }),
            "https://upstream.example",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("secret slot \"private_key\""),
            "err={err}"
        );
    }

    #[test]
    fn request_mapping_accepts_tool_choice_and_metadata_templates() {
        let manifest = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "request": {
                        "path": "/deployments/{{metadata.deployment}}/chat",
                        "embedding_path": "/deployments/{{model}}/embeddings",
                        "query": {
                            "tenant": "{{metadata.tenant}}",
                            "tool": "{{tool_choice}}"
                        },
                        "headers": {
                            "X-Tenant": "{{metadata.tenant}}"
                        },
                        "body": {
                            "messages": "{{messages}}",
                            "tools": "{{tools}}",
                            "toolChoice": "{{tool_choice}}",
                            "metadata": "{{metadata}}"
                        },
                        "embedding_body": {
                            "texts": "{{input_texts}}",
                            "format": "{{encoding_format}}",
                            "dimensions": "{{dimensions}}"
                        }
                    }
                }
            }),
            "https://upstream.example",
        )
        .unwrap();

        assert_eq!(
            manifest.request.path.as_deref(),
            Some("/deployments/{{metadata.deployment}}/chat")
        );
        assert_eq!(
            manifest.request.embedding_path.as_deref(),
            Some("/deployments/{{model}}/embeddings")
        );
        assert!(manifest.request.embedding_body.is_some());
    }

    #[test]
    fn request_mapping_rejects_header_messages_template_variable() {
        let err = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "request": {
                        "headers": {
                            "X-Leak": "{{messages}}"
                        }
                    }
                }
            }),
            "https://upstream.example",
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("unsupported template variable {{messages}}"),
            "err={err}"
        );
    }

    #[test]
    fn response_mapping_accepts_fallback_defaults_and_multimodal_usage_paths() {
        let manifest = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "response": {
                        "openai_compatible": false,
                        "id_path": "missing.id|trace.request_id|default:\"local\"",
                        "model_path": "result.0.model",
                        "content_path": "result.0.text",
                        "reasoning_content_path": "result.0.reasoning",
                        "tool_calls_path": "result.0.tool_calls",
                        "finish_reason_path": "result.0.finish",
                        "request_id_path": "trace.request_id",
                        "metadata_path": "vendor",
                        "usage": {
                            "prompt_tokens_path": "usage.input",
                            "completion_tokens_path": "usage.output",
                            "total_tokens_path": "usage.total|default:0",
                            "cached_tokens_path": "usage.cached",
                            "reasoning_tokens_path": "usage.reasoning",
                            "image_units_path": "usage.images",
                            "audio_seconds_path": "usage.audio_seconds",
                            "raw_path": "usage"
                        }
                    }
                }
            }),
            "https://upstream.example",
        )
        .unwrap();

        assert_eq!(
            manifest.response.request_id_path.as_deref(),
            Some("trace.request_id")
        );
        assert_eq!(
            manifest.response.usage.image_units_path.as_deref(),
            Some("usage.images")
        );
    }

    #[test]
    fn response_mapping_rejects_bracket_array_index() {
        let err = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "response": {
                        "openai_compatible": false,
                        "content_path": "choices[0].message.content"
                    }
                }
            }),
            "https://upstream.example",
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("use dot array indexes"),
            "err={err}"
        );
    }

    #[test]
    fn parses_hmac_auth_manifest_defaults_and_payload_template() {
        let manifest = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "hmac",
                        "secret_slot": "signing-key",
                        "hmac": {
                            "signature_header": "X-Kooix-Signature",
                            "signed_payload": "{{method}}\n{{path}}\n{{body_sha256}}\n{{timestamp}}\n{{nonce}}",
                            "signature_encoding": "base64"
                        }
                    }
                }
            }),
            "https://upstream.example",
        )
        .unwrap();

        assert_eq!(manifest.auth.strategy, AuthStrategy::Hmac);
        assert_eq!(manifest.auth.secret_slot(), "signing-key");
        assert_eq!(manifest.auth.hmac.signature_header, "X-Kooix-Signature");
        assert_eq!(
            manifest.auth.hmac.signature_encoding,
            SignatureEncoding::Base64
        );
        assert_eq!(manifest.auth.hmac.timestamp_header, "X-Timestamp");
        assert_eq!(manifest.auth.hmac.nonce_header, "X-Nonce");
    }

    #[test]
    fn parses_aws_sigv4_auth_manifest_defaults() {
        let manifest = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "aws_sigv4",
                        "aws_sigv4": {
                            "region": "us-east-1"
                        }
                    }
                }
            }),
            "https://bedrock-runtime.us-east-1.amazonaws.com",
        )
        .unwrap();

        assert_eq!(manifest.auth.strategy, AuthStrategy::AwsSigv4);
        assert_eq!(manifest.auth.aws_sigv4.service, "bedrock");
        assert_eq!(manifest.auth.aws_sigv4.secret_key_slot, "aws_secret_key");
        assert_eq!(
            manifest.auth.aws_sigv4.session_token_slot.as_deref(),
            Some("aws_session_token")
        );
    }

    #[test]
    fn parses_oauth_client_credentials_manifest_defaults() {
        let manifest = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "oauth_client_credentials",
                        "oauth": {
                            "token_url": "https://idp.example.com/oauth/token",
                            "scope": "chat:write"
                        }
                    },
                    "security": {
                        "permissions": { "oauth_client_credentials": true }
                    }
                }
            }),
            "https://upstream.example",
        )
        .unwrap();

        assert_eq!(manifest.auth.strategy, AuthStrategy::OauthClientCredentials);
        assert_eq!(
            manifest.auth.oauth.token_url,
            "https://idp.example.com/oauth/token"
        );
        assert_eq!(manifest.auth.oauth.client_id_slot, "client_id");
        assert_eq!(manifest.auth.oauth.client_secret_slot, "client_secret");
        assert_eq!(manifest.auth.oauth.scope.as_deref(), Some("chat:write"));
        assert_eq!(manifest.auth.oauth.token_type, "Bearer");
        assert_eq!(manifest.auth.oauth.expiry_skew_seconds, 60);
    }

    #[test]
    fn oauth_rejects_plain_http_token_url() {
        let err = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "oauth_client_credentials",
                        "oauth": {
                            "token_url": "http://idp.example.com/oauth/token"
                        }
                    },
                    "security": {
                        "permissions": { "oauth_client_credentials": true }
                    }
                }
            }),
            "https://upstream.example",
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("oauth token_url must use https"),
            "err={err}"
        );
    }

    #[test]
    fn bedrock_preset_defaults_to_aws_sigv4_without_fake_secret_headers() {
        let manifest = PluginManifest::from_value(
            json!({ "plugin": { "preset": { "provider": "bedrock_converse" } } }),
            "https://bedrock-runtime.us-east-1.amazonaws.com",
        )
        .unwrap();

        assert_eq!(manifest.auth.strategy, AuthStrategy::AwsSigv4);
        assert!(!manifest.request.headers.contains_key("X-Amz-Access-Key"));
        assert!(!manifest.request.headers.contains_key("X-Amz-Secret-Key"));
        assert_eq!(
            manifest.request.path.as_deref(),
            Some("/model/{{model}}/converse")
        );
    }

    #[test]
    fn preset_defaults_fill_capabilities() {
        let manifest = PluginManifest::from_value(
            json!({ "plugin": { "preset": { "provider": "anthropic_messages" } } }),
            "https://api.anthropic.com",
        )
        .unwrap();

        assert!(manifest.capabilities.chat);
        assert!(manifest.capabilities.streaming);
        assert!(manifest.capabilities.tools);
        assert!(manifest.capabilities.vision);
        assert!(manifest.capabilities.json_mode);
        assert!(!manifest.capabilities.embeddings);
    }

    #[test]
    fn openai_compatible_variant_presets_parse() {
        for provider in [
            "vllm",
            "lm_studio",
            "ollama_openai",
            "localai",
            "xinference",
            "vertex_openai",
        ] {
            let manifest = PluginManifest::from_value(
                json!({ "plugin": { "preset": { "provider": provider } } }),
                "http://localhost:8000/v1",
            )
            .unwrap();

            assert_eq!(
                manifest.request.path.as_deref(),
                Some("/chat/completions"),
                "provider={provider}"
            );
            assert_eq!(
                manifest.request.embedding_path.as_deref(),
                Some("/embeddings"),
                "provider={provider}"
            );
            assert!(manifest.capabilities.chat, "provider={provider}");
            assert!(manifest.capabilities.streaming, "provider={provider}");
            assert!(manifest.capabilities.embeddings, "provider={provider}");
        }
    }

    #[test]
    fn vertex_openai_preset_uses_openai_path_and_bearer_auth() {
        let manifest = PluginManifest::from_value(
            json!({ "plugin": { "preset": { "provider": "vertex_openai" } } }),
            "https://aiplatform.googleapis.com/v1/projects/demo/locations/us-central1/endpoints/openapi",
        )
        .unwrap();

        assert_eq!(manifest.auth.strategy, AuthStrategy::Bearer);
        assert_eq!(manifest.auth.secret_slot.as_deref(), Some("primary"));
        assert_eq!(manifest.request.path.as_deref(), Some("/chat/completions"));
        assert_eq!(
            manifest.request.embedding_path.as_deref(),
            Some("/embeddings")
        );
        assert!(manifest.response.is_openai_compatible());
        assert!(manifest.embedding_response.is_openai_compatible());
        assert!(manifest.stream.is_openai_compatible());
        assert!(manifest.capabilities.chat);
        assert!(manifest.capabilities.streaming);
        assert!(manifest.capabilities.tools);
        assert!(manifest.capabilities.embeddings);
        assert!(manifest.capabilities.vision);
        assert!(manifest.capabilities.json_mode);
    }

    #[test]
    fn hmac_rejects_unknown_payload_template_variable() {
        let err = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "hmac",
                        "hmac": {
                            "signed_payload": "{{api_key}}\n{{body_sha256}}"
                        }
                    }
                }
            }),
            "https://upstream.example",
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("unsupported template variable {{api_key}}"),
            "err={err}"
        );
    }

    #[test]
    fn upgrades_legacy_v0_preset_manifest() {
        let manifest = PluginManifest::from_value(
            json!({ "plugin": { "preset": { "provider": "azure_openai" } } }),
            "https://example.openai.azure.com",
        )
        .unwrap();

        assert_eq!(manifest.version, 1);
        assert!(
            manifest
                .request
                .path
                .unwrap()
                .contains("/openai/deployments/")
        );
        assert_eq!(manifest.auth.strategy, AuthStrategy::ApiKeyHeader);
        assert_eq!(manifest.auth.header_name(), Some("api-key"));
        assert!(!manifest.request.headers.contains_key("Authorization"));
    }

    #[test]
    fn deserialize_error_reports_json_pointer() {
        let err = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "security": { "max_request_bytes": "large" }
                }
            }),
            "https://upstream.example",
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("/plugin/security/max_request_bytes"),
            "err={err}"
        );
    }

    #[test]
    fn validates_probe_manifest_path_body_status_and_cost() {
        let manifest = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "probe": {
                        "model": "tiny-health",
                        "path": "/health/{{model}}",
                        "body": {
                            "model": "{{model}}",
                            "messages": "{{messages}}",
                            "max_tokens": "{{max_tokens}}"
                        },
                        "success_status": [200, 204],
                        "max_cost_micros": 25
                    }
                }
            }),
            "https://upstream.example",
        )
        .unwrap();

        assert_eq!(manifest.probe.model.as_deref(), Some("tiny-health"));
        assert_eq!(manifest.probe.path.as_deref(), Some("/health/{{model}}"));
        assert_eq!(manifest.probe.success_status_or_default(), vec![200, 204]);
        assert_eq!(manifest.probe.max_cost_micros, Some(25));
    }

    #[test]
    fn rejects_invalid_probe_success_status_and_negative_cost() {
        let err = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "probe": { "success_status": [99] }
                }
            }),
            "https://upstream.example",
        )
        .unwrap_err();
        assert!(err.to_string().contains("/plugin/probe/success_status"));

        let err = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "probe": { "max_cost_micros": -1 }
                }
            }),
            "https://upstream.example",
        )
        .unwrap_err();
        assert!(err.to_string().contains("/plugin/probe/max_cost_micros"));
    }

    #[test]
    fn schema_contains_v1_sections() {
        let schema = plugin_manifest_schema_json();
        let props = &schema["$defs"]["PluginManifest"]["properties"];
        for section in [
            "version",
            "metadata",
            "capabilities",
            "auth",
            "request",
            "response",
            "stream",
            "usage",
            "error",
            "probe",
            "security",
        ] {
            assert!(props.get(section).is_some(), "missing {section}");
        }
    }
}
