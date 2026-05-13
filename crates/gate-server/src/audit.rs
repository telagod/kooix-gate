//! AuditEmitter: 非阻塞审计日志发射器
//!
//! 从 `AuthContext` 提取 actor 信息，构造 `AuditRecord` 后 `tokio::spawn` 异步写入。
//! 写入失败仅 warn 不阻断业务操作。

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
}
