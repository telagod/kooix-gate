//! AuditEmitter: 非阻塞审计日志发射器
//!
//! 从 `AuthContext` 提取 actor 信息，构造 `AuditRecord` 后 `tokio::spawn` 异步写入。
//! 写入失败仅 warn 不阻断业务操作。

use crate::audit_redaction::redact_json;
use crate::middleware::KooixRequestId;
use gate_auth::context::{AuthContext, Subject};
use gate_storage::{AuditRecord, AuditRepo};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct AuditEmitter {
    repo: Arc<dyn AuditRepo>,
}

impl AuditEmitter {
    pub fn new(repo: Arc<dyn AuditRepo>) -> Self {
        Self { repo }
    }

    /// 发射一条审计记录（非阻塞）。
    ///
    /// `action` 格式: `"api_key.create"`, `"channel.update"` 等。
    /// `resource_kind`: `"api_key"`, `"channel"`, `"quota"`, `"user"` 等。
    pub fn emit(
        &self,
        ctx: &AuthContext,
        action: &str,
        resource_kind: &str,
        resource_id: Option<Uuid>,
        detail: Option<serde_json::Value>,
    ) {
        let (actor_kind, actor_id) = match ctx.subject() {
            Some(Subject::User { user_id, .. }) => ("user".to_string(), Some(*user_id.as_uuid())),
            Some(Subject::ApiKey { api_key_id, .. }) => {
                ("api_key".to_string(), Some(*api_key_id.as_uuid()))
            }
            Some(Subject::System) => ("system".to_string(), None),
            None => ("system".to_string(), None),
        };

        let org_id = ctx.current_org().map(|o| *o.as_uuid());

        let record = AuditRecord {
            id: Uuid::now_v7(),
            ts: chrono::Utc::now(),
            actor_kind,
            actor_id,
            actor_ip: None,
            actor_user_agent: None,
            request_id: None,
            action: action.to_string(),
            resource_kind: resource_kind.to_string(),
            resource_id,
            org_id,
            project_id: None,
            before: None,
            after: detail,
            outcome: "success".to_string(),
            error_message: None,
        };

        let repo = self.repo.clone();
        let action_owned = action.to_string();
        tokio::spawn(async move {
            if let Err(e) = repo.append(&record).await {
                tracing::warn!(error = %e, action = %action_owned, "audit log write failed");
            }
        });
    }

    /// 发射带 before/after 和 HTTP metadata 的审计记录。
    ///
    /// `before` / `after` 会先经过 secret redaction，避免高危操作 diff 泄露 key/token。
    pub fn emit_change(&self, change: AuditChange<'_>) {
        let AuditChange {
            ctx,
            meta,
            action,
            resource_kind,
            resource_id,
            before,
            after,
        } = change;
        let (actor_kind, actor_id) = actor_from_context(ctx);
        let org_id = ctx.current_org().map(|o| *o.as_uuid());

        let record = AuditRecord {
            id: Uuid::now_v7(),
            ts: chrono::Utc::now(),
            actor_kind,
            actor_id,
            actor_ip: meta.actor_ip,
            actor_user_agent: meta.actor_user_agent,
            request_id: meta.request_id,
            action: action.to_string(),
            resource_kind: resource_kind.to_string(),
            resource_id,
            org_id,
            project_id: meta.project_id,
            before: before.map(redact_json),
            after: after.map(redact_json),
            outcome: "success".to_string(),
            error_message: None,
        };

        let repo = self.repo.clone();
        let action_owned = action.to_string();
        tokio::spawn(async move {
            if let Err(e) = repo.append(&record).await {
                tracing::warn!(error = %e, action = %action_owned, "audit log write failed");
            }
        });
    }
}

#[derive(Debug)]
pub struct AuditChange<'a> {
    pub ctx: &'a AuthContext,
    pub meta: AuditRequestMeta,
    pub action: &'a str,
    pub resource_kind: &'a str,
    pub resource_id: Option<Uuid>,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct AuditRequestMeta {
    pub actor_ip: Option<String>,
    pub actor_user_agent: Option<String>,
    pub request_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
}

impl AuditRequestMeta {
    pub fn from_parts(
        request_id: Option<KooixRequestId>,
        headers: &axum::http::HeaderMap,
        project_id: Option<Uuid>,
    ) -> Self {
        Self {
            actor_ip: extract_actor_ip(headers),
            actor_user_agent: headers
                .get(axum::http::header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.chars().take(512).collect()),
            request_id: request_id.map(|rid| rid.0),
            project_id,
        }
    }
}

fn actor_from_context(ctx: &AuthContext) -> (String, Option<Uuid>) {
    match ctx.subject() {
        Some(Subject::User { user_id, .. }) => ("user".to_string(), Some(*user_id.as_uuid())),
        Some(Subject::ApiKey { api_key_id, .. }) => {
            ("api_key".to_string(), Some(*api_key_id.as_uuid()))
        }
        Some(Subject::System) => ("system".to_string(), None),
        None => ("system".to_string(), None),
    }
}

fn extract_actor_ip(headers: &axum::http::HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(first) = v.split(',').next().map(str::trim).filter(|v| !v.is_empty())
    {
        return Some(first.chars().take(64).collect());
    }
    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.chars().take(64).collect())
}
