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
