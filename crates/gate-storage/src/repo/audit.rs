//! AuditRepo — 审计日志持久层。
//!
//! `audit_logs` 表记录所有配置变更操作。append-only，只写不改。
//! 查询接口提供按 Org / 按 Resource 两个维度的分页列表。

use crate::error::DbResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use sqlx::{PgPool, Row};
use std::collections::VecDeque;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditSortBy {
    Ts,
    ActorKind,
    Action,
    ResourceKind,
    Outcome,
}

impl AuditSortBy {
    pub fn sql_column(self) -> &'static str {
        match self {
            Self::Ts => "ts",
            Self::ActorKind => "actor_kind",
            Self::Action => "action",
            Self::ResourceKind => "resource_kind",
            Self::Outcome => "outcome",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    pub fn sql(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

/// 审计日志行（与 `audit_logs` 表 1:1 映射）。
#[derive(Debug, Clone)]
pub struct AuditRecord {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    pub actor_kind: String,
    pub actor_id: Option<Uuid>,
    pub actor_ip: Option<String>,
    pub actor_user_agent: Option<String>,
    pub request_id: Option<Uuid>,
    pub action: String,
    pub resource_kind: String,
    pub resource_id: Option<Uuid>,
    pub org_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub outcome: String,
    pub error_message: Option<String>,
}

#[async_trait]
pub trait AuditRepo: Send + Sync + 'static {
    /// 追加一条审计记录。
    async fn append(&self, record: &AuditRecord) -> DbResult<()>;

    /// 按 Org 查询审计日志（分页，时间倒序）。
    async fn list_by_org(
        &self,
        org_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> DbResult<Vec<AuditRecord>>;

    /// 按 Org 查询审计日志（分页 + 可控排序）。排序字段必须由调用方枚举化，避免 SQL 注入。
    async fn list_by_org_sorted(
        &self,
        org_id: Uuid,
        limit: i64,
        offset: i64,
        sort_by: AuditSortBy,
        sort_dir: SortDirection,
    ) -> DbResult<Vec<AuditRecord>>;

    /// 按资源查询审计日志（分页，时间倒序）。
    async fn list_by_resource(
        &self,
        resource_kind: &str,
        resource_id: Uuid,
        limit: i64,
    ) -> DbResult<Vec<AuditRecord>>;
}

// ============================================================================
// PgAuditRepo
// ============================================================================

pub struct PgAuditRepo {
    pool: PgPool,
}

impl PgAuditRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const AUDIT_COLUMNS: &str = "id, ts, actor_kind, actor_id, actor_ip::TEXT, actor_user_agent, \
    request_id, action, resource_kind, resource_id, org_id, project_id, \
    before, after, outcome, error_message";

fn row_to_record(row: &sqlx::postgres::PgRow) -> DbResult<AuditRecord> {
    Ok(AuditRecord {
        id: row.try_get("id")?,
        ts: row.try_get("ts")?,
        actor_kind: row.try_get("actor_kind")?,
        actor_id: row.try_get("actor_id")?,
        actor_ip: row.try_get::<Option<String>, _>("actor_ip")?,
        actor_user_agent: row.try_get("actor_user_agent")?,
        request_id: row.try_get("request_id")?,
        action: row.try_get("action")?,
        resource_kind: row.try_get("resource_kind")?,
        resource_id: row.try_get("resource_id")?,
        org_id: row.try_get("org_id")?,
        project_id: row.try_get("project_id")?,
        before: row.try_get("before")?,
        after: row.try_get("after")?,
        outcome: row.try_get("outcome")?,
        error_message: row.try_get("error_message")?,
    })
}

#[async_trait]
impl AuditRepo for PgAuditRepo {
    async fn append(&self, r: &AuditRecord) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO audit_logs \
             (id, ts, actor_kind, actor_id, actor_ip, actor_user_agent, request_id, \
              action, resource_kind, resource_id, org_id, project_id, \
              before, after, outcome, error_message) \
             VALUES ($1, $2, $3, $4, $5::INET, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
        )
        .bind(r.id)
        .bind(r.ts)
        .bind(&r.actor_kind)
        .bind(r.actor_id)
        .bind(&r.actor_ip)
        .bind(&r.actor_user_agent)
        .bind(r.request_id)
        .bind(&r.action)
        .bind(&r.resource_kind)
        .bind(r.resource_id)
        .bind(r.org_id)
        .bind(r.project_id)
        .bind(&r.before)
        .bind(&r.after)
        .bind(&r.outcome)
        .bind(&r.error_message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_by_org(
        &self,
        org_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> DbResult<Vec<AuditRecord>> {
        self.list_by_org_sorted(org_id, limit, offset, AuditSortBy::Ts, SortDirection::Desc)
            .await
    }

    async fn list_by_org_sorted(
        &self,
        org_id: Uuid,
        limit: i64,
        offset: i64,
        sort_by: AuditSortBy,
        sort_dir: SortDirection,
    ) -> DbResult<Vec<AuditRecord>> {
        let sort_col = sort_by.sql_column();
        let sort_dir = sort_dir.sql();
        let rows = sqlx::query(&format!(
            "SELECT {AUDIT_COLUMNS} FROM audit_logs \
             WHERE org_id = $1 ORDER BY {sort_col} {sort_dir}, id DESC LIMIT $2 OFFSET $3"
        ))
        .bind(org_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_record).collect()
    }

    async fn list_by_resource(
        &self,
        resource_kind: &str,
        resource_id: Uuid,
        limit: i64,
    ) -> DbResult<Vec<AuditRecord>> {
        let rows = sqlx::query(&format!(
            "SELECT {AUDIT_COLUMNS} FROM audit_logs \
             WHERE resource_kind = $1 AND resource_id = $2 \
             ORDER BY ts DESC LIMIT $3"
        ))
        .bind(resource_kind)
        .bind(resource_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_record).collect()
    }
}

// ============================================================================
// InMemoryAuditRepo（dev 模式 / 测试用）
// ============================================================================

#[derive(Default)]
pub struct InMemoryAuditRepo {
    inner: RwLock<VecDeque<AuditRecord>>,
}

impl InMemoryAuditRepo {
    pub fn new() -> Self {
        Self::default()
    }

    /// 测试用：获取所有记录拷贝。
    pub fn all(&self) -> Vec<AuditRecord> {
        self.inner.read().iter().cloned().collect()
    }
}

#[async_trait]
impl AuditRepo for InMemoryAuditRepo {
    async fn append(&self, record: &AuditRecord) -> DbResult<()> {
        self.inner.write().push_back(record.clone());
        Ok(())
    }

    async fn list_by_org(
        &self,
        org_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> DbResult<Vec<AuditRecord>> {
        self.list_by_org_sorted(org_id, limit, offset, AuditSortBy::Ts, SortDirection::Desc)
            .await
    }

    async fn list_by_org_sorted(
        &self,
        org_id: Uuid,
        limit: i64,
        offset: i64,
        sort_by: AuditSortBy,
        sort_dir: SortDirection,
    ) -> DbResult<Vec<AuditRecord>> {
        let g = self.inner.read();
        let mut rows = g
            .iter()
            .rev()
            .filter(|r| r.org_id == Some(org_id))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            let ordering = match sort_by {
                AuditSortBy::Ts => a.ts.cmp(&b.ts),
                AuditSortBy::ActorKind => a.actor_kind.cmp(&b.actor_kind),
                AuditSortBy::Action => a.action.cmp(&b.action),
                AuditSortBy::ResourceKind => a.resource_kind.cmp(&b.resource_kind),
                AuditSortBy::Outcome => a.outcome.cmp(&b.outcome),
            };
            let ordering = match sort_dir {
                SortDirection::Asc => ordering,
                SortDirection::Desc => ordering.reverse(),
            };
            ordering.then_with(|| b.id.cmp(&a.id))
        });
        Ok(rows
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect())
    }

    async fn list_by_resource(
        &self,
        resource_kind: &str,
        resource_id: Uuid,
        limit: i64,
    ) -> DbResult<Vec<AuditRecord>> {
        let g = self.inner.read();
        Ok(g.iter()
            .rev()
            .filter(|r| r.resource_kind == resource_kind && r.resource_id == Some(resource_id))
            .take(limit as usize)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_append_and_list_by_org() {
        let repo = InMemoryAuditRepo::new();
        let org = Uuid::now_v7();

        let r1 = AuditRecord {
            id: Uuid::now_v7(),
            ts: Utc::now(),
            actor_kind: "user".into(),
            actor_id: Some(Uuid::now_v7()),
            actor_ip: None,
            actor_user_agent: None,
            request_id: None,
            action: "api_key.create".into(),
            resource_kind: "api_key".into(),
            resource_id: Some(Uuid::now_v7()),
            org_id: Some(org),
            project_id: None,
            before: None,
            after: Some(serde_json::json!({"name": "test"})),
            outcome: "success".into(),
            error_message: None,
        };
        repo.append(&r1).await.unwrap();

        // Different org — should not appear
        let r2 = AuditRecord {
            id: Uuid::now_v7(),
            org_id: Some(Uuid::now_v7()),
            action: "channel.create".into(),
            ..r1.clone()
        };
        repo.append(&r2).await.unwrap();

        let results = repo.list_by_org(org, 50, 0).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "api_key.create");
    }

    #[tokio::test]
    async fn in_memory_list_by_resource() {
        let repo = InMemoryAuditRepo::new();
        let res_id = Uuid::now_v7();

        for i in 0..5 {
            let r = AuditRecord {
                id: Uuid::now_v7(),
                ts: Utc::now(),
                actor_kind: "user".into(),
                actor_id: None,
                actor_ip: None,
                actor_user_agent: None,
                request_id: None,
                action: format!("channel.update.{i}"),
                resource_kind: "channel".into(),
                resource_id: Some(res_id),
                org_id: None,
                project_id: None,
                before: None,
                after: None,
                outcome: "success".into(),
                error_message: None,
            };
            repo.append(&r).await.unwrap();
        }

        let results = repo.list_by_resource("channel", res_id, 3).await.unwrap();
        assert_eq!(results.len(), 3);
        // Reverse order (newest first)
        assert_eq!(results[0].action, "channel.update.4");
    }

    #[tokio::test]
    async fn in_memory_offset_pagination() {
        let repo = InMemoryAuditRepo::new();
        let org = Uuid::now_v7();

        for i in 0..10 {
            let r = AuditRecord {
                id: Uuid::now_v7(),
                ts: Utc::now(),
                actor_kind: "user".into(),
                actor_id: None,
                actor_ip: None,
                actor_user_agent: None,
                request_id: None,
                action: format!("action.{i}"),
                resource_kind: "test".into(),
                resource_id: None,
                org_id: Some(org),
                project_id: None,
                before: None,
                after: None,
                outcome: "success".into(),
                error_message: None,
            };
            repo.append(&r).await.unwrap();
        }

        let page1 = repo.list_by_org(org, 3, 0).await.unwrap();
        let page2 = repo.list_by_org(org, 3, 3).await.unwrap();
        assert_eq!(page1.len(), 3);
        assert_eq!(page2.len(), 3);
        assert_eq!(page1[0].action, "action.9");
        assert_eq!(page2[0].action, "action.6");
    }

    #[tokio::test]
    async fn in_memory_sorted_list_is_stable_and_paginated() {
        let repo = InMemoryAuditRepo::new();
        let org = Uuid::now_v7();
        let base = Utc::now();

        for (i, action) in ["zeta", "alpha", "beta", "alpha"].iter().enumerate() {
            let r = AuditRecord {
                id: Uuid::now_v7(),
                ts: base + chrono::Duration::seconds(i as i64),
                actor_kind: "user".into(),
                actor_id: None,
                actor_ip: None,
                actor_user_agent: None,
                request_id: None,
                action: (*action).into(),
                resource_kind: "test".into(),
                resource_id: None,
                org_id: Some(org),
                project_id: None,
                before: None,
                after: None,
                outcome: if i % 2 == 0 { "success" } else { "denied" }.into(),
                error_message: None,
            };
            repo.append(&r).await.unwrap();
        }

        let asc = repo
            .list_by_org_sorted(org, 10, 0, AuditSortBy::Action, SortDirection::Asc)
            .await
            .unwrap();
        assert_eq!(
            asc.iter().map(|r| r.action.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "alpha", "beta", "zeta"]
        );

        let page = repo
            .list_by_org_sorted(org, 2, 1, AuditSortBy::Action, SortDirection::Desc)
            .await
            .unwrap();
        assert_eq!(
            page.iter().map(|r| r.action.as_str()).collect::<Vec<_>>(),
            vec!["beta", "alpha"]
        );
    }
}
