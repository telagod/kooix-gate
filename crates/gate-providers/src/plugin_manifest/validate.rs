//! Plugin manifest validation — auth / metadata / outbound / template / secret slot / response / stream / probe / limits。
//!
//! 入口：`super::PluginManifest::validate` 把 manifest 各段交给本模块的 validate_* 链。
//! Helper：placeholder_allowed / template_paths / required_secret_slots / 等。

use super::helpers::escape_json_pointer;
use super::*;
use crate::error::{ProviderError, ProviderResult};

pub(super) fn validate_metadata_urls(
    metadata: &MetadataManifest,
    pointer_base: &str,
) -> ProviderResult<()> {
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

pub(super) fn validate_outbound_allowlist_entry(entry: &str, pointer: &str) -> ProviderResult<()> {
    let entry = entry.trim();
    if entry.is_empty() {
        return Err(ProviderError::Config(format!(
            "invalid plugin manifest at {pointer}: allowlist entry cannot be empty"
        )));
    }
    let normalized = if entry.contains("://") {
        entry.to_string()
    } else {
        format!("https://{entry}")
    };
    let parsed = reqwest::Url::parse(&normalized).map_err(|e| {
        ProviderError::Config(format!(
            "invalid plugin manifest at {pointer}: invalid allowlist entry {entry:?}: {e}"
        ))
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ProviderError::Config(format!(
            "invalid plugin manifest at {pointer}: allowlist scheme must be http/https"
        )));
    }
    if parsed.host_str().is_none() {
        return Err(ProviderError::Config(format!(
            "invalid plugin manifest at {pointer}: allowlist entry missing host"
        )));
    }
    if parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(ProviderError::Config(format!(
            "invalid plugin manifest at {pointer}: allowlist entries are origins only"
        )));
    }
    Ok(())
}

pub(super) fn validate_header_name(name: &str, pointer: &str) -> ProviderResult<()> {
    reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
        ProviderError::Config(format!(
            "invalid plugin manifest at {pointer}: invalid header name {name:?}: {e}"
        ))
    })?;
    Ok(())
}

pub(super) fn validate_timeout_ms(
    timeout_ms: Option<u64>,
    pointer_base: &str,
) -> ProviderResult<()> {
    let Some(timeout_ms) = timeout_ms else {
        return Ok(());
    };
    if !(1..=HARD_MAX_TIMEOUT_MS).contains(&timeout_ms) {
        return Err(config_at(
            pointer_base,
            "/request/timeout_ms",
            "timeout_ms must be 1..600000",
        ));
    }
    Ok(())
}

pub(super) fn validate_auth(auth: &AuthManifest, pointer_base: &str) -> ProviderResult<()> {
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

pub(super) fn validate_response_paths(
    response: &ResponseManifest,
    pointer: &str,
) -> ProviderResult<()> {
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

pub(super) fn validate_embedding_response_paths(
    response: &EmbeddingResponseManifest,
    pointer: &str,
) -> ProviderResult<()> {
    for (suffix, path) in [
        ("/object_path", response.object_path.as_deref()),
        ("/data_path", response.data_path.as_deref()),
        ("/embedding_path", response.embedding_path.as_deref()),
        ("/index_path", response.index_path.as_deref()),
        ("/model_path", response.model_path.as_deref()),
        (
            "/usage/prompt_tokens_path",
            response.usage.prompt_tokens_path.as_deref(),
        ),
        (
            "/usage/total_tokens_path",
            response.usage.total_tokens_path.as_deref(),
        ),
        ("/usage/raw_path", response.usage.raw_path.as_deref()),
    ] {
        if let Some(path) = path {
            validate_mapping_path(path, &json_pointer(pointer, suffix))?;
        }
    }
    Ok(())
}

pub(super) fn validate_stream_paths(stream: &StreamManifest, pointer: &str) -> ProviderResult<()> {
    for (suffix, path) in [
        ("/event_path", stream.event_path.as_deref()),
        ("/done_path", stream.done_path.as_deref()),
        ("/id_path", stream.id_path.as_deref()),
        ("/model_path", stream.model_path.as_deref()),
        ("/role_path", stream.role_path.as_deref()),
        ("/content_path", stream.content_path.as_deref()),
        ("/tool_calls_path", stream.tool_calls_path.as_deref()),
        ("/finish_reason_path", stream.finish_reason_path.as_deref()),
        (
            "/usage/prompt_tokens_path",
            stream.usage.prompt_tokens_path.as_deref(),
        ),
        (
            "/usage/completion_tokens_path",
            stream.usage.completion_tokens_path.as_deref(),
        ),
        (
            "/usage/total_tokens_path",
            stream.usage.total_tokens_path.as_deref(),
        ),
        (
            "/usage/cached_tokens_path",
            stream.usage.cached_tokens_path.as_deref(),
        ),
        (
            "/usage/reasoning_tokens_path",
            stream.usage.reasoning_tokens_path.as_deref(),
        ),
        (
            "/usage/image_units_path",
            stream.usage.image_units_path.as_deref(),
        ),
        (
            "/usage/audio_seconds_path",
            stream.usage.audio_seconds_path.as_deref(),
        ),
        ("/usage/raw_path", stream.usage.raw_path.as_deref()),
    ] {
        if let Some(path) = path {
            validate_mapping_path(path, &json_pointer(pointer, suffix))?;
        }
    }
    Ok(())
}

pub(super) fn validate_probe(probe: &ProbeManifest, pointer: &str) -> ProviderResult<()> {
    if let Some(path) = &probe.path {
        validate_template_str(path, TemplateScope::Path, &json_pointer(pointer, "/path"))?;
    }
    if let Some(body) = &probe.body {
        validate_template_value(body, TemplateScope::Body, &json_pointer(pointer, "/body"))?;
    }
    for status in &probe.success_status {
        if !(100..=599).contains(status) {
            return Err(ProviderError::Config(format!(
                "invalid plugin manifest at {}: HTTP status must be 100..599",
                json_pointer(pointer, "/success_status")
            )));
        }
    }
    if let Some(max_cost_micros) = probe.max_cost_micros
        && max_cost_micros < 0
    {
        return Err(ProviderError::Config(format!(
            "invalid plugin manifest at {}: must be >= 0",
            json_pointer(pointer, "/max_cost_micros")
        )));
    }
    Ok(())
}

pub(super) fn validate_mapping_path(expr: &str, pointer: &str) -> ProviderResult<()> {
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

pub(super) fn header_belongs_to_auth(auth: &AuthManifest, header: &str) -> bool {
    auth.header_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(header))
        || auth
            .headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case(header))
}

pub(super) fn validate_secret_slot(
    pointer_base: &str,
    suffix: &str,
    slot: &str,
) -> ProviderResult<()> {
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

pub(crate) fn required_secret_slots(auth: &AuthManifest) -> Vec<String> {
    let mut slots = Vec::new();
    match auth.strategy {
        AuthStrategy::Bearer | AuthStrategy::ApiKeyHeader | AuthStrategy::ApiKeyQuery => {
            push_secret_slot(&mut slots, auth.secret_slot());
        }
        AuthStrategy::Basic => {
            if let Some(slot) = auth.username_slot() {
                push_secret_slot(&mut slots, slot);
            }
            push_secret_slot(
                &mut slots,
                auth.password_slot().unwrap_or_else(|| auth.secret_slot()),
            );
        }
        AuthStrategy::CustomHeaders => {
            push_secret_slot(&mut slots, auth.secret_slot());
        }
        AuthStrategy::Hmac => {
            push_secret_slot(&mut slots, auth.secret_slot());
        }
        AuthStrategy::AwsSigv4 => {
            push_secret_slot(&mut slots, &auth.aws_sigv4.access_key_slot);
            push_secret_slot(&mut slots, &auth.aws_sigv4.secret_key_slot);
            if let Some(slot) = auth.aws_sigv4.session_token_slot.as_deref() {
                push_secret_slot(&mut slots, slot);
            }
        }
        AuthStrategy::OauthClientCredentials => {
            push_secret_slot(&mut slots, &auth.oauth.client_id_slot);
            push_secret_slot(&mut slots, &auth.oauth.client_secret_slot);
        }
        AuthStrategy::None => {}
    }
    slots
}

pub(super) fn push_secret_slot(slots: &mut Vec<String>, slot: &str) {
    let normalized = if slot.trim().is_empty() || slot.eq_ignore_ascii_case("api_key") {
        "primary".to_string()
    } else {
        slot.to_ascii_lowercase()
    };
    if !slots.iter().any(|existing| existing == &normalized) {
        slots.push(normalized);
    }
}

pub(super) fn secret_slot_declared(declared: &[String], required: &str) -> bool {
    declared.iter().any(|slot| {
        let mut normalized = Vec::new();
        push_secret_slot(&mut normalized, slot);
        normalized.iter().any(|slot| slot == required)
    })
}

pub(super) fn reject_plain_secret_strings(
    auth: &AuthManifest,
    pointer_base: &str,
) -> ProviderResult<()> {
    let value = serde_json::to_value(auth).map_err(|e| ProviderError::Config(e.to_string()))?;
    walk_plain_secret_strings(&value, pointer_base, "/auth")
}

pub(super) fn walk_plain_secret_strings(
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

pub(super) fn is_local_test_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

#[derive(Debug, Clone, Copy)]
pub(super) enum TemplateScope {
    Path,
    Query,
    Header,
    Body,
    Hmac,
}

pub(super) fn validate_template_value(
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

pub(super) fn validate_template_str(
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

pub(super) fn template_paths(template: &str) -> Vec<String> {
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

pub(super) fn placeholder_allowed(scope: TemplateScope, path: &str) -> bool {
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
                    | "input"
                    | "input_texts"
                    | "stream"
                    | "temperature"
                    | "top_p"
                    | "max_tokens"
                    | "encoding_format"
                    | "dimensions"
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
                    | "input"
                    | "input_texts"
                    | "stream"
                    | "temperature"
                    | "top_p"
                    | "max_tokens"
                    | "encoding_format"
                    | "dimensions"
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
                    | "input"
                    | "input_texts"
                    | "messages"
                    | "metadata"
                    | "extra"
                    | "tools"
                    | "tool_choice"
                    | "stream"
                    | "temperature"
                    | "top_p"
                    | "max_tokens"
                    | "encoding_format"
                    | "dimensions"
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

pub(super) fn validate_limit(name: &str, value: usize, hard_max: usize) -> ProviderResult<()> {
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
