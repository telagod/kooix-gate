//! ModelAliasRepo — 模型别名持久层。
//!
//! model_aliases 表（migration 05）：
//! - project_id + alias → target_model 映射
//! - enabled=TRUE 才生效
//! - 可选 group_id 强制路由 + params_override 参数覆盖（本期不消费，预留）

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gate_core::id::ProjectId;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::sync::RwLock;
use uuid::Uuid;

/// model_aliases 行快照。
#[derive(Debug, Clone)]
pub struct ModelAliasRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub alias: String,
    pub target_model: String,
    pub group_id: Option<Uuid>,
    pub params_override: serde_json::Value,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Resolved alias result with optional parameter overrides.
#[derive(Debug, Clone)]
pub struct ResolvedAlias {
    pub target_model: String,
    pub params_override: serde_json::Value,
}

#[async_trait]
pub trait ModelAliasRepo: Send + Sync + 'static {
    /// 解析 alias → target_model + params_override。只查 enabled=TRUE 的行。
    /// 返回 None 表示无匹配 alias（用原始 model）。
    async fn resolve(
        &self,
        project_id: ProjectId,
        requested_model: &str,
    ) -> DbResult<Option<ResolvedAlias>>;

    /// 列出 project 下所有 alias（含 disabled），admin 视图用。
    async fn list_by_project(&self, project_id: ProjectId) -> DbResult<Vec<ModelAliasRecord>>;

    /// UPSERT：(project_id, alias) 命中则更新 target_model。
    async fn upsert(
        &self,
        project_id: ProjectId,
        alias: &str,
        target_model: &str,
    ) -> DbResult<()>;

    /// 删除一条 alias（硬删）。
    async fn delete(&self, project_id: ProjectId, alias: &str) -> DbResult<()>;
}

// ============================================================================
// PgModelAliasRepo
// ============================================================================

pub struct PgModelAliasRepo {
    pool: PgPool,
}

impl PgModelAliasRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const ALIAS_COLUMNS: &str = "id, project_id, alias, target_model, group_id, \
    params_override, enabled, created_at, updated_at";

fn row_to_record(row: &sqlx::postgres::PgRow) -> DbResult<ModelAliasRecord> {
    Ok(ModelAliasRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        alias: row.try_get("alias")?,
        target_model: row.try_get("target_model")?,
        group_id: row.try_get("group_id")?,
        params_override: row.try_get("params_override")?,
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait]
impl ModelAliasRepo for PgModelAliasRepo {
    async fn resolve(
        &self,
        project_id: ProjectId,
        requested_model: &str,
    ) -> DbResult<Option<ResolvedAlias>> {
        let row = sqlx::query(
            "SELECT target_model, params_override FROM model_aliases \
             WHERE project_id = $1 AND alias = $2 AND enabled = TRUE",
        )
        .bind(project_id.as_uuid())
        .bind(requested_model)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => {
                let target: String = r.try_get("target_model")?;
                let params: serde_json::Value = r.try_get("params_override").unwrap_or(serde_json::json!({}));
                Ok(Some(ResolvedAlias { target_model: target, params_override: params }))
            }
            None => Ok(None),
        }
    }

    async fn list_by_project(&self, project_id: ProjectId) -> DbResult<Vec<ModelAliasRecord>> {
        let rows = sqlx::query(&format!(
            "SELECT {ALIAS_COLUMNS} FROM model_aliases \
             WHERE project_id = $1 ORDER BY alias ASC"
        ))
        .bind(project_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_record).collect()
    }

    async fn upsert(
        &self,
        project_id: ProjectId,
        alias: &str,
        target_model: &str,
    ) -> DbResult<()> {
        // 先查存在性
        let existing = sqlx::query(
            "SELECT id FROM model_aliases WHERE project_id = $1 AND alias = $2",
        )
        .bind(project_id.as_uuid())
        .bind(alias)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = existing {
            let id: Uuid = row.try_get("id")?;
            sqlx::query(
                "UPDATE model_aliases SET target_model = $1, enabled = TRUE \
                 WHERE id = $2",
            )
            .bind(target_model)
            .bind(id)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO model_aliases (project_id, alias, target_model, enabled) \
                 VALUES ($1, $2, $3, TRUE)",
            )
            .bind(project_id.as_uuid())
            .bind(alias)
            .bind(target_model)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn delete(&self, project_id: ProjectId, alias: &str) -> DbResult<()> {
        let res = sqlx::query(
            "DELETE FROM model_aliases WHERE project_id = $1 AND alias = $2",
        )
        .bind(project_id.as_uuid())
        .bind(alias)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}

// ============================================================================
// InMemoryModelAliasRepo（dev 模式 / 测试用）
// ============================================================================

#[derive(Default)]
pub struct InMemoryModelAliasRepo {
    inner: RwLock<HashMap<(Uuid, String), ModelAliasRecord>>,
}

impl InMemoryModelAliasRepo {
    pub fn new() -> Self {
        Self::default()
    }

    /// 测试快捷：直接 seed 一行。
    pub fn seed(&self, record: ModelAliasRecord) {
        self.inner
            .write()
            .unwrap()
            .insert((record.project_id, record.alias.clone()), record);
    }
}

#[async_trait]
impl ModelAliasRepo for InMemoryModelAliasRepo {
    async fn resolve(
        &self,
        project_id: ProjectId,
        requested_model: &str,
    ) -> DbResult<Option<ResolvedAlias>> {
        let key = (*project_id.as_uuid(), requested_model.to_string());
        Ok(self
            .inner
            .read()
            .unwrap()
            .get(&key)
            .filter(|r| r.enabled)
            .map(|r| ResolvedAlias {
                target_model: r.target_model.clone(),
                params_override: r.params_override.clone(),
            }))
    }

    async fn list_by_project(&self, project_id: ProjectId) -> DbResult<Vec<ModelAliasRecord>> {
        let pid = *project_id.as_uuid();
        let mut out: Vec<ModelAliasRecord> = self
            .inner
            .read()
            .unwrap()
            .values()
            .filter(|r| r.project_id == pid)
            .cloned()
            .collect();
        out.sort_by(|a, b| a.alias.cmp(&b.alias));
        Ok(out)
    }

    async fn upsert(
        &self,
        project_id: ProjectId,
        alias: &str,
        target_model: &str,
    ) -> DbResult<()> {
        let pid = *project_id.as_uuid();
        let key = (pid, alias.to_string());
        let now = Utc::now();
        let mut g = self.inner.write().unwrap();

        if let Some(existing) = g.get_mut(&key) {
            existing.target_model = target_model.to_string();
            existing.enabled = true;
            existing.updated_at = now;
        } else {
            g.insert(
                key,
                ModelAliasRecord {
                    id: Uuid::now_v7(),
                    project_id: pid,
                    alias: alias.to_string(),
                    target_model: target_model.to_string(),
                    group_id: None,
                    params_override: serde_json::json!({}),
                    enabled: true,
                    created_at: now,
                    updated_at: now,
                },
            );
        }
        Ok(())
    }

    async fn delete(&self, project_id: ProjectId, alias: &str) -> DbResult<()> {
        let key = (*project_id.as_uuid(), alias.to_string());
        if self.inner.write().unwrap().remove(&key).is_none() {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}
