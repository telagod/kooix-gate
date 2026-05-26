//! 跨 admin/{channels,groups,sso,users,probe,invitations,pricing,org_members} 子
//! 模块共享的 helper。
//!
//! 0.4.151-0.4.155（第四刀第 1 项真还）：原本 13 个 helper 长在 channels.rs，
//! 让 sibling 反向依赖 channels.rs。本文件物理收容：
//!
//! - 0.4.151: 空骨架
//! - 0.4.152: confirmation_from_headers / require_confirmation / audit_meta
//! - 0.4.153: channel/key/group/pricing_rule/user audit_snapshot（5 fn）
//! - 0.4.154: is_plugin_provider / channel_capabilities / record_to_summary /
//!            channel_inflight / key_fingerprint / validate_channel_key_alias
//! - 0.4.155: sibling 改 `use super::shared::{...}`，消除 sibling → channels 反向依赖
//!
//! channels.rs 仍保留 13 个 thin wrapper（`super::shared::xxx`）供自身 handler 调用。

#![allow(unused_imports)]

use super::*;

// ============================================================================
// 0.4.152 迁入第 1 批：confirmation + audit_meta（3 fn）
// 原位置: crates/gate-server/src/routes/admin/channels.rs:164-187
// ============================================================================

pub(super) fn confirmation_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(CONFIRM_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

pub(super) fn require_confirmation(
    headers: &HeaderMap,
    expected: impl AsRef<str>,
) -> AppResult<()> {
    let expected = expected.as_ref();
    match confirmation_from_headers(headers) {
        Some(actual) if actual == expected => Ok(()),
        _ => Err(AppError::BadRequest(format!(
            "confirmation required: set {CONFIRM_HEADER}: {expected}"
        ))),
    }
}

pub(super) fn audit_meta(
    request_id: Option<Extension<KooixRequestId>>,
    headers: &HeaderMap,
) -> AuditRequestMeta {
    AuditRequestMeta::from_parts(request_id.map(|Extension(id)| id), headers, None)
}

// ============================================================================
// 0.4.153 迁入第 2 批：5 个 audit_snapshot
// 原位置: channels.rs:86-162
// ============================================================================

pub(super) fn channel_audit_snapshot(r: &gate_storage::ChannelRecord) -> serde_json::Value {
    serde_json::json!({
        "id": r.channel_id.to_string(),
        "code": r.code,
        "name": r.name,
        "provider_type": r.provider_type,
        "base_url": r.base_url,
        "status": r.status,
        "health": r.health,
        "supported_models": r.supported_models,
        "rpm_limit": r.rpm_limit,
        "tpm_limit": r.tpm_limit,
        "timeout_ms": r.timeout_ms,
        "max_retries": r.max_retries,
        "tags": r.tags,
        "model_mapping": r.model_mapping,
        "balance": r.balance,
        "last_error": r.last_error,
    })
}

pub(super) fn key_audit_snapshot(k: &gate_storage::ChannelKeyRecord) -> serde_json::Value {
    serde_json::json!({
        "id": k.id.to_string(),
        "channel_id": k.channel_id.to_string(),
        "label": k.label,
        "fingerprint": k.key_fingerprint,
        "weight": k.weight,
        "health": k.health,
        "total_requests": k.total_requests,
        "total_errors": k.total_errors,
        "consecutive_errors": k.consecutive_errors,
        "last_error_code": k.last_error_code,
    })
}

pub(super) fn group_audit_snapshot(
    g: &gate_storage::ChannelGroupRecord,
    channel_count: i64,
) -> serde_json::Value {
    serde_json::json!({
        "id": g.group_id.to_string(),
        "name": g.name,
        "description": g.description,
        "strategy": g.strategy,
        "enabled": g.enabled,
        "fallback_group_id": g.fallback_group_id.map(|fb| fb.to_string()),
        "channel_count": channel_count,
    })
}

pub(super) fn pricing_rule_audit_snapshot(r: &gate_billing::PricingRule) -> serde_json::Value {
    serde_json::json!({
        "id": r.id.to_string(),
        "channel_id": r.channel_id.map(|c| gate_core::id::ChannelId::from(c).to_string()),
        "model": r.model,
        "dimension": r.dimension,
        "unit": r.unit,
        "rate": r.rate,
        "conditions": r.conditions,
        "effective_from": r.effective_from,
        "effective_until": r.effective_until,
        "priority": r.priority,
        "description": r.description,
    })
}

pub(super) fn user_audit_snapshot(u: &gate_core::identity::User) -> serde_json::Value {
    serde_json::json!({
        "id": u.id.to_string(),
        "email": u.email,
        "display_name": u.display_name,
        "status": format!("{:?}", u.status).to_lowercase(),
        "mfa_enabled": u.mfa_enabled,
        "last_login_at": u.last_login_at,
    })
}

// ============================================================================
// 0.4.154 迁入第 3 批：channel/key 6 个 helper
// 原位置: channels.rs:59 / 124 / 225 / 377 / 502 / 508
// ============================================================================

pub(super) fn is_plugin_provider(provider_type: &str) -> bool {
    matches!(provider_type, "plugin" | "custom" | "http" | "http_plugin")
}

pub(super) fn channel_capabilities(r: &gate_storage::ChannelRecord) -> ProviderCapabilities {
    if is_plugin_provider(&r.provider_type) {
        return gate_providers::plugin_manifest(r.model_mapping.clone(), &r.base_url)
            .map(|manifest| manifest.capabilities)
            .unwrap_or_else(|_| gate_providers::provider_capabilities(&r.provider_type));
    }
    gate_providers::provider_capabilities(&r.provider_type)
}

pub(super) fn record_to_summary(r: gate_storage::ChannelRecord) -> ChannelSummary {
    let capabilities = channel_capabilities(&r);
    ChannelSummary {
        id: r.channel_id.to_string(),
        code: r.code,
        name: r.name,
        provider_type: r.provider_type,
        base_url: r.base_url,
        status: r.status,
        health: r.health,
        supported_models: r.supported_models,
        rpm_limit: r.rpm_limit,
        tpm_limit: r.tpm_limit,
        timeout_ms: r.timeout_ms,
        max_retries: r.max_retries,
        tags: r.tags,
        capabilities,
        model_mapping: r.model_mapping,
        balance: r.balance,
        balance_updated_at: r.balance_updated_at,
        last_error: r.last_error,
        last_error_at: r.last_error_at,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

pub(super) fn channel_inflight(app: &AppState, channel_id: ChannelId) -> i64 {
    app.provider_router
        .as_ref()
        .map(|router| router.inflight_tracker().current(channel_id))
        .unwrap_or(0)
}

/// 计算 key fingerprint：SHA-256 前 16 字节 hex。
pub(super) fn key_fingerprint(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(secret.as_bytes());
    hex::encode(&hash[..16])
}

pub(super) fn validate_channel_key_alias(alias: &str) -> AppResult<()> {
    let alias = alias.trim();
    if alias.is_empty() {
        return Ok(());
    }
    if !alias
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AppError::BadRequest(
            "key alias must use [a-zA-Z0-9_-]".into(),
        ));
    }
    Ok(())
}
