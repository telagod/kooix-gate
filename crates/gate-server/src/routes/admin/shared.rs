//! 跨 admin/{channels,groups,sso,users,probe,invitations,pricing,org_members} 子
//! 模块共享的 helper。
//!
//! 0.4.151（按 0.4.130 推 v0.5.x 项第 1 项真还）：第三刀的 channels.rs 既装
//! 业务 handler 又是共享 helper 库，让 sibling 反向依赖 channels.rs。
//! 本文件先建骨架，0.4.152-154 分批迁入 13 个共享 helper：
//!
//! - `require_confirmation` / `confirmation_from_headers` / `audit_meta`
//! - `channel_audit_snapshot` / `key_audit_snapshot` / `group_audit_snapshot`
//!   / `pricing_rule_audit_snapshot` / `user_audit_snapshot`
//! - `channel_capabilities` / `channel_inflight` / `is_plugin_provider`
//! - `key_fingerprint` / `validate_channel_key_alias` / `record_to_summary`
//!
//! 迁完后 (0.4.155) sibling 改用 `use super::shared::{...}` 替代
//! `use super::channels::{...}`，消除 sibling → channels 的事实反向依赖。

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
