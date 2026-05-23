//! Strongly typed HTTP plugin manifest.
//!
//! Runtime accepts the legacy v0 shape (`{ "plugin": { "preset": ... } }`) and
//! upgrades it into the v1 internal representation. New manifests should use
//! `plugin.version = 1` with fixed sections so backend validation, web forms and
//! CLI lint share one contract.

mod factory;
mod helpers;
mod upgrade;
mod validate;

pub use factory::{
    plugin_manifest, plugin_manifest_retry_config, plugin_manifest_schema_json,
    validate_plugin_manifest,
};

use crate::error::ProviderResult;
use crate::plugin_preset::{
    EmbeddingResponseManifest, PresetManifest, ProviderPresetSpec, ResponseManifest,
    StreamManifest, UsageManifest,
};
use helpers::{config_at, json_pointer, path_to_json_pointer};
use schemars::JsonSchema;
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
    /// ADR-0002 fast-path runtime 标志：若为 true，runtime 走静态分发优化路径，
    /// 跳过 manifest 解释器以达到 ≤ × 1.02 性能预算。
    ///
    /// **不允许用户 manifest 设置**：用户传入的 `security.builtin_fastpath` 在
    /// `from_value` 解析后会被 `enforce_user_manifest_safety` 强制清零，只有
    /// `plugin_preset.rs` 静态注册时（4 个 fast-path provider）才能注入 true。
    /// 这是为了防止下游通过 channel.model_mapping 关掉 fast-path 或反之
    /// 把不安全的非 fast-path provider 强制走未实现的路径。
    ///
    /// 字段在 0.4.0 接 dispatch 实现；0.3.x 仅接 schema + 注入点。
    pub builtin_fastpath: bool,
    /// ADR-0003 WASM Plugin ABI v0：可选 wasm transform module 配置。
    ///
    /// 0.4.23 起作为 typed field 接 manifest schema；runtime 接入在
    /// gate-providers 集成层落地（0.4.24-0.4.25 接 chat / response / stream hook）。
    pub wasm: Option<WasmModuleManifest>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(default)]
pub struct WasmModuleManifest {
    /// Module 文件路径或注册表 ID（runtime 解析后加载）。
    pub module: String,
    /// SHA256 期望摘要（hex），加载时强校验。
    pub module_sha256: String,
    /// Linear memory 上限（bytes），默认 ADR-0003 v0 hard limit 16 MiB。
    pub max_memory_bytes: Option<usize>,
    /// 单次 hook CPU 上限（ms），默认 50ms。
    pub max_cpu_ms: Option<u64>,
    /// 启用的 hook 集合，子集 ∈ {chat_request_transform, chat_response_transform, stream_chunk_transform}。
    pub hooks: Vec<String>,
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

        // ADR-0002: builtin_fastpath 必须由 preset 静态注入，不允许用户 channel
        // manifest 设置（防止下游绕过 dispatch 锁定）。任何用户传入的值在这里清零，
        // 真正的 fast-path 只在 apply_preset 里给 4 个白名单 provider 打开。
        manifest.security.builtin_fastpath = false;

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

        // ADR-0002: 静态注入 builtin_fastpath。0.3.x 仅设置 schema 字段，dispatch
        // 实现在 0.4.0 落地（CustomHttpProvider 内部见 `self.manifest.security.builtin_fastpath`）。
        if matches!(
            kind,
            crate::plugin_preset::ProviderPresetKind::Openai
                | crate::plugin_preset::ProviderPresetKind::AnthropicMessages
                | crate::plugin_preset::ProviderPresetKind::AzureOpenai
                | crate::plugin_preset::ProviderPresetKind::BedrockConverse
        ) {
            self.security.builtin_fastpath = true;
        }

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

#[cfg(test)]
mod tests;
