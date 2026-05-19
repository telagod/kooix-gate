//! Strongly typed HTTP plugin manifest.
//!
//! Runtime accepts the legacy v0 shape (`{ "plugin": { "preset": ... } }`) and
//! upgrades it into the v1 internal representation. New manifests should use
//! `plugin.version = 1` with fixed sections so backend validation, web forms and
//! CLI lint share one contract.

use crate::error::{ProviderError, ProviderResult};
use crate::plugin_preset::{
    PresetManifest, ProviderPresetSpec, ResponseManifest, StreamManifest, UsageManifest,
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub(crate) const DEFAULT_CHAT_PATH: &str = "/chat/completions";
pub(crate) const DEFAULT_MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub(crate) const DEFAULT_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const DEFAULT_MAX_SSE_EVENT_BYTES: usize = 1024 * 1024;
const HARD_MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
const HARD_MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const HARD_MAX_SSE_EVENT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub(crate) struct ChannelPluginMapping {
    pub plugin: PluginManifest,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub(crate) struct PluginManifest {
    pub version: u8,
    pub metadata: MetadataManifest,
    pub capabilities: CapabilitiesManifest,
    pub auth: AuthManifest,
    pub request: RequestManifest,
    pub response: ResponseManifest,
    pub stream: StreamManifest,
    pub usage: UsageManifest,
    pub error: ErrorManifest,
    pub probe: ProbeManifest,
    pub security: SecurityManifest,
    pub(crate) preset: PresetManifest,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub(crate) struct MetadataManifest {
    pub name: Option<String>,
    pub vendor: Option<String>,
    pub homepage: Option<String>,
    pub docs: Option<String>,
    pub owner: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub(crate) struct CapabilitiesManifest {
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

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthStrategy {
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
pub(crate) struct AuthManifest {
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

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HmacAlgorithm {
    #[default]
    Sha256,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub(crate) struct HmacAuthManifest {
    pub algorithm: HmacAlgorithm,
    pub signature_header: String,
    pub timestamp_header: String,
    pub nonce_header: String,
    pub signed_payload: String,
    pub signature_encoding: SignatureEncoding,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SignatureEncoding {
    Base64,
    #[default]
    Hex,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub(crate) struct AwsSigv4AuthManifest {
    pub service: String,
    pub region: Option<String>,
    pub access_key_slot: String,
    pub secret_key_slot: String,
    pub session_token_slot: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub(crate) struct OauthClientCredentialsManifest {
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
pub(crate) enum RequestMethod {
    Get,
    #[default]
    Post,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub(crate) struct RequestManifest {
    pub method: RequestMethod,
    #[serde(alias = "chat_path")]
    pub path: Option<String>,
    pub query: Map<String, Value>,
    pub headers: Map<String, Value>,
    pub body: Option<Value>,
    pub timeout_ms: Option<u64>,
    pub retry: RetryManifest,
    pub force_stream_field: bool,
    pub stream_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub(crate) struct RetryManifest {
    pub max_retries: Option<u8>,
    pub retryable_status: Vec<u16>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub(crate) struct ErrorManifest {
    pub status_path: Option<String>,
    pub code_path: Option<String>,
    pub message_path: Option<String>,
    pub retryable_status: Vec<u16>,
    pub retryable_codes: Vec<String>,
    pub cooldown_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub(crate) struct ProbeManifest {
    pub model: Option<String>,
    pub path: Option<String>,
    pub body: Option<Value>,
    pub success_status: Vec<u16>,
    pub max_cost_micros: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub(crate) struct SecurityManifest {
    pub outbound_allowlist: Vec<String>,
    pub header_redaction: Vec<String>,
    pub max_request_bytes: Option<usize>,
    pub max_response_bytes: Option<usize>,
    pub max_sse_event_bytes: Option<usize>,
    pub allow_absolute_chat_path: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct LegacyPluginManifest {
    preset: PresetManifest,
    request: LegacyRequestManifest,
    response: ResponseManifest,
    stream: StreamManifest,
    security: SecurityManifest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct LegacyRequestManifest {
    chat_path: Option<String>,
    headers: Map<String, Value>,
    body: Option<Value>,
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
        if self.request.body.is_none() {
            self.request.body = spec.body;
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
        validate_response_paths(&self.response, &json_pointer(pointer_base, "/response"))?;
        self.security.validate(pointer_base)
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
            query: Map::new(),
            headers: Map::new(),
            body: None,
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

pub fn validate_plugin_manifest(value: Value, base_url: &str) -> ProviderResult<()> {
    PluginManifest::from_value(value, base_url).map(|_| ())
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

fn extract_plugin_value(value: Value) -> (Value, String) {
    if let Some(v) = value.get("plugin") {
        (v.clone(), "/plugin".to_string())
    } else if let Some(v) = value.get("adapter") {
        (v.clone(), "/adapter".to_string())
    } else if let Some(v) = value.get("protocol") {
        (v.clone(), "/protocol".to_string())
    } else {
        (value, String::new())
    }
}

fn deserialize_v1(value: Value, pointer_base: &str) -> ProviderResult<PluginManifest> {
    let input = value.to_string();
    let mut de = serde_json::Deserializer::from_str(&input);
    serde_path_to_error::deserialize::<_, PluginManifest>(&mut de).map_err(|err| {
        let pointer = json_pointer(pointer_base, &path_to_json_pointer(err.path()));
        ProviderError::Config(format!(
            "invalid plugin manifest at {pointer}: {}",
            err.inner()
        ))
    })
}

fn upgrade_v0(value: Value, pointer_base: &str) -> ProviderResult<PluginManifest> {
    let input = value.to_string();
    let mut de = serde_json::Deserializer::from_str(&input);
    let legacy =
        serde_path_to_error::deserialize::<_, LegacyPluginManifest>(&mut de).map_err(|err| {
            let pointer = json_pointer(pointer_base, &path_to_json_pointer(err.path()));
            ProviderError::Config(format!(
                "invalid legacy plugin manifest at {pointer}: {}",
                err.inner()
            ))
        })?;

    Ok(PluginManifest {
        preset: legacy.preset,
        request: RequestManifest {
            path: legacy.request.chat_path,
            headers: legacy.request.headers,
            body: legacy.request.body,
            force_stream_field: legacy.request.force_stream_field,
            stream_path: legacy.request.stream_path,
            ..Default::default()
        },
        response: legacy.response,
        stream: legacy.stream,
        security: legacy.security,
        ..Default::default()
    })
}

impl Default for LegacyRequestManifest {
    fn default() -> Self {
        Self {
            chat_path: None,
            headers: Map::new(),
            body: None,
            force_stream_field: true,
            stream_path: "stream".to_string(),
        }
    }
}

fn validate_metadata_urls(metadata: &MetadataManifest, pointer_base: &str) -> ProviderResult<()> {
    for (name, value) in [
        ("homepage", metadata.homepage.as_deref()),
        ("docs", metadata.docs.as_deref()),
    ] {
        let Some(url) = value else {
            continue;
        };
        let parsed = reqwest::Url::parse(url).map_err(|e| {
            ProviderError::Config(format!(
                "invalid plugin manifest at {}: {e}",
                json_pointer(pointer_base, &format!("/metadata/{name}"))
            ))
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(config_at(
                pointer_base,
                &format!("/metadata/{name}"),
                "URL scheme must be http/https",
            ));
        }
    }
    Ok(())
}

fn validate_auth(auth: &AuthManifest, pointer_base: &str) -> ProviderResult<()> {
    match auth.strategy {
        AuthStrategy::ApiKeyHeader => {
            if auth.header_name.as_deref().unwrap_or("").trim().is_empty() {
                return Err(config_at(
                    pointer_base,
                    "/auth/header_name",
                    "api_key_header auth requires header_name",
                ));
            }
        }
        AuthStrategy::ApiKeyQuery => {
            if auth.query_name.as_deref().unwrap_or("").trim().is_empty() {
                return Err(config_at(
                    pointer_base,
                    "/auth/query_name",
                    "api_key_query auth requires query_name",
                ));
            }
        }
        AuthStrategy::Basic => {
            if auth
                .username_slot
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                return Err(config_at(
                    pointer_base,
                    "/auth/username_slot",
                    "basic auth requires username_slot",
                ));
            }
            if let Some(username_slot) = auth.username_slot() {
                validate_secret_slot(pointer_base, "/auth/username_slot", username_slot)?;
            }
            if let Some(password_slot) = auth.password_slot() {
                validate_secret_slot(pointer_base, "/auth/password_slot", password_slot)?;
            }
        }
        AuthStrategy::CustomHeaders => {
            if auth.headers.is_empty() {
                return Err(config_at(
                    pointer_base,
                    "/auth/headers",
                    "custom_headers auth requires at least one header",
                ));
            }
            for (name, value) in &auth.headers {
                validate_template_value(
                    value,
                    TemplateScope::Header,
                    &json_pointer(pointer_base, &format!("/auth/headers/{name}")),
                )?;
            }
        }
        AuthStrategy::Hmac => {
            if auth.hmac.signature_header.trim().is_empty() {
                return Err(config_at(
                    pointer_base,
                    "/auth/hmac/signature_header",
                    "hmac auth requires signature_header",
                ));
            }
            if auth.hmac.timestamp_header.trim().is_empty() {
                return Err(config_at(
                    pointer_base,
                    "/auth/hmac/timestamp_header",
                    "hmac auth requires timestamp_header",
                ));
            }
            if auth.hmac.nonce_header.trim().is_empty() {
                return Err(config_at(
                    pointer_base,
                    "/auth/hmac/nonce_header",
                    "hmac auth requires nonce_header",
                ));
            }
            validate_template_str(
                &auth.hmac.signed_payload,
                TemplateScope::Hmac,
                &json_pointer(pointer_base, "/auth/hmac/signed_payload"),
            )?;
        }
        AuthStrategy::AwsSigv4 => {
            if auth.aws_sigv4.service.trim().is_empty() {
                return Err(config_at(
                    pointer_base,
                    "/auth/aws_sigv4/service",
                    "aws_sigv4 auth requires service",
                ));
            }
            if let Some(region) = auth.aws_sigv4.region.as_deref()
                && region.trim().is_empty()
            {
                return Err(config_at(
                    pointer_base,
                    "/auth/aws_sigv4/region",
                    "aws_sigv4 region must not be empty when provided",
                ));
            }
            validate_secret_slot(
                pointer_base,
                "/auth/aws_sigv4/access_key_slot",
                &auth.aws_sigv4.access_key_slot,
            )?;
            validate_secret_slot(
                pointer_base,
                "/auth/aws_sigv4/secret_key_slot",
                &auth.aws_sigv4.secret_key_slot,
            )?;
            if let Some(slot) = auth.aws_sigv4.session_token_slot.as_deref() {
                validate_secret_slot(pointer_base, "/auth/aws_sigv4/session_token_slot", slot)?;
            }
        }
        AuthStrategy::OauthClientCredentials => {
            let token_url = auth.oauth.token_url.trim();
            if token_url.is_empty() {
                return Err(config_at(
                    pointer_base,
                    "/auth/oauth/token_url",
                    "oauth_client_credentials auth requires token_url",
                ));
            }
            let parsed = reqwest::Url::parse(token_url).map_err(|e| {
                ProviderError::Config(format!(
                    "invalid plugin manifest at {}: invalid oauth token_url: {e}",
                    json_pointer(pointer_base, "/auth/oauth/token_url")
                ))
            })?;
            let is_test_http_local = cfg!(test)
                && parsed.scheme() == "http"
                && parsed.host_str().is_some_and(is_local_test_host);
            if parsed.scheme() != "https" && !is_test_http_local {
                return Err(config_at(
                    pointer_base,
                    "/auth/oauth/token_url",
                    "oauth token_url must use https",
                ));
            }
            validate_secret_slot(
                pointer_base,
                "/auth/oauth/client_id_slot",
                &auth.oauth.client_id_slot,
            )?;
            validate_secret_slot(
                pointer_base,
                "/auth/oauth/client_secret_slot",
                &auth.oauth.client_secret_slot,
            )?;
            if auth.oauth.token_type.trim().is_empty() {
                return Err(config_at(
                    pointer_base,
                    "/auth/oauth/token_type",
                    "oauth token_type must not be empty",
                ));
            }
            if auth.oauth.expiry_skew_seconds > 3600 {
                return Err(config_at(
                    pointer_base,
                    "/auth/oauth/expiry_skew_seconds",
                    "oauth expiry_skew_seconds must be <= 3600",
                ));
            }
        }
        AuthStrategy::Bearer | AuthStrategy::None => {}
    }
    if let Some(secret_slot) = auth.secret_slot.as_deref() {
        validate_secret_slot(pointer_base, "/auth/secret_slot", secret_slot)?;
    }
    reject_plain_secret_strings(auth, pointer_base)
}

fn validate_response_paths(response: &ResponseManifest, pointer: &str) -> ProviderResult<()> {
    for (suffix, path) in [
        ("/id_path", response.id_path.as_deref()),
        ("/model_path", response.model_path.as_deref()),
        ("/content_path", response.content_path.as_deref()),
        (
            "/reasoning_content_path",
            response.reasoning_content_path.as_deref(),
        ),
        ("/tool_calls_path", response.tool_calls_path.as_deref()),
        (
            "/finish_reason_path",
            response.finish_reason_path.as_deref(),
        ),
        ("/request_id_path", response.request_id_path.as_deref()),
        ("/metadata_path", response.metadata_path.as_deref()),
        (
            "/usage/prompt_tokens_path",
            response.usage.prompt_tokens_path.as_deref(),
        ),
        (
            "/usage/completion_tokens_path",
            response.usage.completion_tokens_path.as_deref(),
        ),
        (
            "/usage/total_tokens_path",
            response.usage.total_tokens_path.as_deref(),
        ),
        (
            "/usage/cached_tokens_path",
            response.usage.cached_tokens_path.as_deref(),
        ),
        (
            "/usage/reasoning_tokens_path",
            response.usage.reasoning_tokens_path.as_deref(),
        ),
        (
            "/usage/image_units_path",
            response.usage.image_units_path.as_deref(),
        ),
        (
            "/usage/audio_seconds_path",
            response.usage.audio_seconds_path.as_deref(),
        ),
        ("/usage/raw_path", response.usage.raw_path.as_deref()),
    ] {
        if let Some(path) = path {
            validate_mapping_path(path, &json_pointer(pointer, suffix))?;
        }
    }
    Ok(())
}

fn validate_mapping_path(expr: &str, pointer: &str) -> ProviderResult<()> {
    let mut has_path = false;
    let mut has_default = false;
    for segment in expr.split('|').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(literal) = segment
            .strip_prefix("default:")
            .or_else(|| segment.strip_prefix("literal:"))
        {
            has_default = true;
            serde_json::from_str::<Value>(literal.trim()).map_err(|e| {
                ProviderError::Config(format!(
                    "invalid plugin manifest at {pointer}: invalid literal default {literal:?}: {e}"
                ))
            })?;
            continue;
        }
        has_path = true;
        for part in segment.trim_start_matches("$.").split('.') {
            if part.is_empty() {
                continue;
            }
            if part.contains('[') || part.contains(']') {
                return Err(ProviderError::Config(format!(
                    "invalid plugin manifest at {pointer}: use dot array indexes like choices.0.message.content"
                )));
            }
        }
    }
    if !has_path && !has_default {
        return Err(ProviderError::Config(format!(
            "invalid plugin manifest at {pointer}: mapping path cannot be empty"
        )));
    }
    Ok(())
}

fn header_belongs_to_auth(auth: &AuthManifest, header: &str) -> bool {
    auth.header_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(header))
        || auth
            .headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case(header))
}

fn validate_secret_slot(pointer_base: &str, suffix: &str, slot: &str) -> ProviderResult<()> {
    if !slot
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(config_at(
            pointer_base,
            suffix,
            "secret slot must use [a-zA-Z0-9_-]",
        ));
    }
    Ok(())
}

fn reject_plain_secret_strings(auth: &AuthManifest, pointer_base: &str) -> ProviderResult<()> {
    let value = serde_json::to_value(auth).map_err(|e| ProviderError::Config(e.to_string()))?;
    walk_plain_secret_strings(&value, pointer_base, "/auth")
}

fn walk_plain_secret_strings(
    value: &Value,
    pointer_base: &str,
    pointer: &str,
) -> ProviderResult<()> {
    match value {
        Value::String(s) => {
            let lower = s.to_ascii_lowercase();
            let looks_secret = lower.starts_with("sk-")
                || lower.starts_with("ak")
                || lower.contains("secret")
                || lower.contains("token");
            let is_reference = s.starts_with("{{") && s.ends_with("}}");
            let is_slot_field = pointer.ends_with("_slot")
                || pointer.ends_with("/secret_slot")
                || pointer.ends_with("/header_name")
                || pointer.ends_with("/query_name");
            let is_oauth_metadata = matches!(
                pointer,
                "/auth/oauth/token_url"
                    | "/auth/oauth/token_type"
                    | "/auth/oauth/scope"
                    | "/auth/oauth/audience"
            );
            if looks_secret && !is_reference && !is_slot_field && !is_oauth_metadata {
                return Err(config_at(
                    pointer_base,
                    pointer,
                    "auth must reference encrypted secret slots or template variables, not plaintext secrets",
                ));
            }
        }
        Value::Array(items) => {
            for (idx, item) in items.iter().enumerate() {
                walk_plain_secret_strings(item, pointer_base, &format!("{pointer}/{idx}"))?;
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                walk_plain_secret_strings(
                    item,
                    pointer_base,
                    &format!("{pointer}/{}", escape_json_pointer(key)),
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_local_test_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

#[derive(Debug, Clone, Copy)]
enum TemplateScope {
    Path,
    Query,
    Header,
    Body,
    Hmac,
}

fn validate_template_value(
    value: &Value,
    scope: TemplateScope,
    pointer: &str,
) -> ProviderResult<()> {
    match value {
        Value::String(s) => validate_template_str(s, scope, pointer),
        Value::Array(arr) => {
            for (idx, item) in arr.iter().enumerate() {
                validate_template_value(item, scope, &format!("{pointer}/{idx}"))?;
            }
            Ok(())
        }
        Value::Object(obj) => {
            for (key, item) in obj {
                validate_template_value(
                    item,
                    scope,
                    &format!("{pointer}/{}", escape_json_pointer(key)),
                )?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_template_str(
    template: &str,
    scope: TemplateScope,
    pointer: &str,
) -> ProviderResult<()> {
    for path in template_paths(template) {
        if !placeholder_allowed(scope, &path) {
            return Err(ProviderError::Config(format!(
                "invalid plugin manifest at {pointer}: unsupported template variable {{{{{path}}}}}"
            )));
        }
    }
    Ok(())
}

fn template_paths(template: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            break;
        };
        paths.push(after_start[..end].trim().to_string());
        rest = &after_start[end + 2..];
    }
    paths
}

fn placeholder_allowed(scope: TemplateScope, path: &str) -> bool {
    let path = path.trim().trim_start_matches("$.").trim_start_matches('$');
    if path.is_empty() {
        return false;
    }
    match scope {
        TemplateScope::Header => {
            matches!(
                path,
                "api_key"
                    | "aws_secret_key"
                    | "aws_session_token"
                    | "model"
                    | "stream"
                    | "temperature"
                    | "top_p"
                    | "max_tokens"
                    | "tools"
                    | "tool_choice"
            ) || path.starts_with("metadata.")
                || path.starts_with("extra.")
        }
        TemplateScope::Path | TemplateScope::Query => {
            matches!(
                path,
                "api_key"
                    | "aws_secret_key"
                    | "aws_session_token"
                    | "model"
                    | "stream"
                    | "temperature"
                    | "top_p"
                    | "max_tokens"
                    | "last_user_message"
                    | "tools"
                    | "tool_choice"
            ) || path.starts_with("request.")
                || path.starts_with("metadata.")
                || path.starts_with("extra.")
        }
        TemplateScope::Body => {
            matches!(
                path,
                "api_key"
                    | "aws_secret_key"
                    | "aws_session_token"
                    | "model"
                    | "messages"
                    | "metadata"
                    | "extra"
                    | "tools"
                    | "tool_choice"
                    | "stream"
                    | "temperature"
                    | "top_p"
                    | "max_tokens"
                    | "last_user_message"
            ) || path.starts_with("request.")
                || path.starts_with("messages.")
                || path.starts_with("metadata.")
                || path.starts_with("extra.")
        }
        TemplateScope::Hmac => {
            matches!(
                path,
                "method" | "path" | "query" | "body" | "body_sha256" | "timestamp" | "nonce"
            ) || path.starts_with("request.")
        }
    }
}

fn validate_limit(name: &str, value: usize, hard_max: usize) -> ProviderResult<()> {
    if value == 0 {
        return Err(ProviderError::Config(format!(
            "invalid plugin manifest at {name}: must be greater than 0"
        )));
    }
    if value > hard_max {
        return Err(ProviderError::Config(format!(
            "invalid plugin manifest at {name}: must be <= {hard_max} bytes"
        )));
    }
    Ok(())
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
                    "response": { "openai_compatible": false, "content_path": "answer" },
                    "stream": { "openai_compatible": false, "content_path": "token" },
                    "usage": { "prompt_tokens_path": "usage.prompt" },
                    "error": { "message_path": "error.message" },
                    "probe": { "model": "tiny", "success_status": [200] },
                    "security": { "max_request_bytes": 4096, "header_redaction": ["authorization"] }
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
        assert_eq!(manifest.auth.strategy, AuthStrategy::ApiKeyHeader);
    }

    #[test]
    fn request_mapping_accepts_tool_choice_and_metadata_templates() {
        let manifest = PluginManifest::from_value(
            json!({
                "plugin": {
                    "version": 1,
                    "request": {
                        "path": "/deployments/{{metadata.deployment}}/chat",
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
