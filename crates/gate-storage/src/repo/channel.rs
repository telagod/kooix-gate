//! ChannelRepo + ChannelGroupRepo — 上游连接与路由分组。
//!
//! 注意：channel_keys 表暂不涉及（Provider 用 env 占位），
//! 但 channel.config_enc 也暂不解密——调用方只用 base_url 构造 Provider。

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gate_core::id::{ChannelGroupId, ChannelId, ProjectId};
use sqlx::{PgPool, Row};
use uuid::Uuid;

// ============================================================================
// Channel
// ============================================================================

/// 渠道快照（不含密钥）。
#[derive(Debug, Clone)]
pub struct ChannelRecord {
    pub channel_id: ChannelId,
    pub code: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub supported_models: Vec<String>,
    pub status: String,
    pub health: String,
    pub timeout_ms: i32,
    pub max_retries: i32,
    /// NULL = 无限制
    pub rpm_limit: Option<i32>,
    /// NULL = 无限制
    pub tpm_limit: Option<i32>,
    pub tags: Vec<String>,
    pub model_mapping: serde_json::Value,
    pub balance: Option<f64>,
    pub balance_updated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 分页/过滤请求参数。
#[derive(Debug, Clone, Default)]
pub struct ListChannelsQuery {
    pub search: Option<String>,
    pub provider: Option<String>,
    pub status: Option<String>,
    pub health: Option<String>,
    pub tag: Option<String>,
    pub page: i64,
    pub page_size: i64,
    pub sort_by: String,
    pub sort_dir: String,
}

/// 分页结果。
#[derive(Debug, Clone)]
pub struct PaginatedChannels {
    pub data: Vec<ChannelRecord>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

impl ChannelRecord {
    pub fn is_healthy(&self) -> bool {
        self.status == "active" && self.health == "healthy"
    }
}

/// 带路由权重的渠道快照（用于 ProviderRouter 选路）。
#[derive(Debug, Clone)]
pub struct ChannelBinding {
    pub channel: ChannelRecord,
    /// 数字越小优先级越高。
    pub priority: i32,
    pub weight: i32,
    /// Binding-level model filter. Empty = no restriction (use channel.supported_models).
    pub model_filter: Vec<String>,
    /// Whether this binding is enabled.
    pub enabled: bool,
}

/// 创建 Channel 的入参。
#[derive(Debug, Clone)]
pub struct CreateChannel {
    pub code: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub supported_models: Vec<String>,
    pub enabled: bool,
    pub rpm_limit: Option<i32>,
    pub tpm_limit: Option<i32>,
    pub timeout_ms: Option<i32>,
    pub max_retries: Option<i32>,
    pub tags: Vec<String>,
    pub model_mapping: Option<serde_json::Value>,
}

/// 更新 Channel 的入参（全部可选，None 表示不改）。
#[derive(Debug, Clone)]
pub struct UpdateChannel {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub supported_models: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub rpm_limit: Option<i32>,
    pub tpm_limit: Option<i32>,
    pub timeout_ms: Option<i32>,
    pub max_retries: Option<i32>,
    pub tags: Option<Vec<String>>,
    pub model_mapping: Option<serde_json::Value>,
}

/// Channel control-plane status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelStatus {
    Active,
    Draining,
    Disabled,
}

impl ChannelStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Draining => "draining",
            Self::Disabled => "disabled",
        }
    }
}

#[async_trait]
pub trait ChannelRepo: Send + Sync + 'static {
    /// 按 ID 查渠道。
    async fn find_by_id(&self, id: ChannelId) -> DbResult<ChannelRecord>;

    /// 查一个 Group 内所有 healthy + enabled 的渠道（含路由权重），按 priority ASC 排序。
    async fn list_healthy_in_group(
        &self,
        group_id: ChannelGroupId,
    ) -> DbResult<Vec<ChannelBinding>>;

    /// 列出全部 channels（admin 视图，含未健康/disabled）。
    /// 控制台只读用，不返回密钥/config 字段。
    async fn list_admin_view(&self) -> DbResult<Vec<ChannelRecord>>;

    /// 分页+过滤+排序列出 channels（admin 视图）。
    async fn list_admin_paginated(&self, query: ListChannelsQuery) -> DbResult<PaginatedChannels>;

    /// 创建新 channel（admin 操作）。
    async fn create(&self, input: CreateChannel) -> DbResult<ChannelRecord>;

    /// 更新 channel 字段（admin 操作）。
    async fn update(&self, id: ChannelId, input: UpdateChannel) -> DbResult<ChannelRecord>;

    /// 更新 channel control-plane status（admin/draining 操作）。
    async fn set_status(&self, id: ChannelId, status: ChannelStatus) -> DbResult<ChannelRecord>;

    /// 软删除（设 deleted_at + status='disabled'）。
    async fn soft_delete(&self, id: ChannelId) -> DbResult<()>;

    /// 自动禁用渠道（成功率低于阈值时）：设 status='disabled', health='unhealthy', 记 last_error。
    async fn auto_disable(&self, id: ChannelId, reason: &str) -> DbResult<()>;

    /// 恢复渠道（健康探活成功后）：设 status='active', health='healthy', 清 last_error。
    async fn re_enable(&self, id: ChannelId) -> DbResult<()>;

    /// 批量更新 enabled 状态。
    async fn batch_set_enabled(&self, ids: &[ChannelId], enabled: bool) -> DbResult<u64>;

    /// 批量软删除。
    async fn batch_soft_delete(&self, ids: &[ChannelId]) -> DbResult<u64>;

    /// 同步模型列表（health check 自动发现用）。
    /// 仅当 discovered 非空且与当前不同时才更新。
    async fn sync_models(&self, id: ChannelId, models: &[String]) -> DbResult<bool>;
}

pub struct PgChannelRepo {
    pool: PgPool,
}

impl PgChannelRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_channel(row: &sqlx::postgres::PgRow) -> DbResult<ChannelRecord> {
    let id: Uuid = row.try_get("id")?;
    Ok(ChannelRecord {
        channel_id: ChannelId::from(id),
        code: row.try_get("code")?,
        name: row.try_get("name")?,
        provider_type: row.try_get("provider_type")?,
        base_url: row.try_get("base_url")?,
        supported_models: row.try_get("supported_models")?,
        status: row.try_get("status")?,
        health: row.try_get("health")?,
        timeout_ms: row.try_get("timeout_ms")?,
        max_retries: row.try_get("max_retries")?,
        rpm_limit: row.try_get("rpm_limit")?,
        tpm_limit: row.try_get("tpm_limit")?,
        tags: row.try_get("tags").unwrap_or_default(),
        model_mapping: row
            .try_get("model_mapping")
            .unwrap_or(serde_json::Value::Object(Default::default())),
        balance: row.try_get("balance").unwrap_or(None),
        balance_updated_at: row.try_get("balance_updated_at").unwrap_or(None),
        last_error: row.try_get("last_error").unwrap_or(None),
        last_error_at: row.try_get("last_error_at").unwrap_or(None),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait]
impl ChannelRepo for PgChannelRepo {
    async fn find_by_id(&self, id: ChannelId) -> DbResult<ChannelRecord> {
        let row = sqlx::query(
            "SELECT id, code, name, provider_type, base_url, supported_models, \
                    status, health, timeout_ms, max_retries, rpm_limit, tpm_limit, \
                    tags, model_mapping, balance, balance_updated_at, \
                    last_error, last_error_at, created_at, updated_at \
             FROM channels WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_channel(&row)
    }

    async fn list_healthy_in_group(
        &self,
        group_id: ChannelGroupId,
    ) -> DbResult<Vec<ChannelBinding>> {
        let rows = sqlx::query(
            "SELECT c.id, c.code, c.name, c.provider_type, c.base_url, c.supported_models, \
                    c.status, c.health, c.timeout_ms, c.max_retries, c.rpm_limit, c.tpm_limit, \
                    c.tags, c.model_mapping, c.balance, c.balance_updated_at, \
                    c.last_error, c.last_error_at, c.created_at, c.updated_at, \
                    b.priority, b.weight, b.model_filter \
             FROM channel_group_bindings b \
             JOIN channels c ON c.id = b.channel_id \
             WHERE b.group_id = $1 \
               AND b.enabled = TRUE \
               AND c.status = 'active' \
               AND c.health = 'healthy' \
               AND c.deleted_at IS NULL \
             ORDER BY b.priority ASC, b.weight DESC",
        )
        .bind(group_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;

        rows.iter()
            .map(|r| {
                Ok(ChannelBinding {
                    channel: row_to_channel(r)?,
                    priority: r.try_get("priority")?,
                    weight: r.try_get("weight")?,
                    model_filter: r.try_get("model_filter").unwrap_or_default(),
                    enabled: true, // this query only fetches enabled bindings
                })
            })
            .collect()
    }

    async fn list_admin_view(&self) -> DbResult<Vec<ChannelRecord>> {
        let rows = sqlx::query(
            "SELECT id, code, name, provider_type, base_url, supported_models, \
                    status, health, timeout_ms, max_retries, rpm_limit, tpm_limit, \
                    tags, model_mapping, balance, balance_updated_at, \
                    last_error, last_error_at, created_at, updated_at \
             FROM channels WHERE deleted_at IS NULL \
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_channel).collect()
    }

    async fn list_admin_paginated(&self, q: ListChannelsQuery) -> DbResult<PaginatedChannels> {
        let valid_sorts = [
            "code",
            "name",
            "provider_type",
            "status",
            "health",
            "created_at",
            "updated_at",
        ];
        let sort_col = if valid_sorts.contains(&q.sort_by.as_str()) {
            &q.sort_by
        } else {
            "created_at"
        };
        let sort_dir = if q.sort_dir.eq_ignore_ascii_case("desc") {
            "DESC"
        } else {
            "ASC"
        };
        let page = q.page.max(1);
        let page_size = q.page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        let mut conditions = vec!["deleted_at IS NULL".to_string()];
        let mut bind_idx = 1u32;
        let mut bind_values: Vec<String> = Vec::new();

        if let Some(ref search) = q.search
            && !search.is_empty()
        {
            bind_idx += 1;
            conditions.push(format!(
                "(code ILIKE ${bind_idx} OR name ILIKE ${bind_idx})"
            ));
            bind_values.push(format!("%{search}%"));
        }
        if let Some(ref provider) = q.provider
            && !provider.is_empty()
        {
            bind_idx += 1;
            conditions.push(format!("provider_type = ${bind_idx}"));
            bind_values.push(provider.clone());
        }
        if let Some(ref status) = q.status
            && !status.is_empty()
        {
            bind_idx += 1;
            conditions.push(format!("status = ${bind_idx}"));
            bind_values.push(status.clone());
        }
        if let Some(ref health) = q.health
            && !health.is_empty()
        {
            bind_idx += 1;
            conditions.push(format!("health = ${bind_idx}"));
            bind_values.push(health.clone());
        }
        if let Some(ref tag) = q.tag
            && !tag.is_empty()
        {
            bind_idx += 1;
            conditions.push(format!("${bind_idx} = ANY(tags)"));
            bind_values.push(tag.clone());
        }

        let where_clause = conditions.join(" AND ");
        let count_sql = format!("SELECT COUNT(*) as cnt FROM channels WHERE {where_clause}");
        let data_sql = format!(
            "SELECT id, code, name, provider_type, base_url, supported_models, \
                    status, health, timeout_ms, max_retries, rpm_limit, tpm_limit, \
                    tags, model_mapping, balance, balance_updated_at, \
                    last_error, last_error_at, created_at, updated_at \
             FROM channels WHERE {where_clause} \
             ORDER BY {sort_col} {sort_dir} \
             LIMIT {page_size} OFFSET {offset}"
        );

        // Build count query with bindings
        let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
        for v in &bind_values {
            count_q = count_q.bind(v);
        }
        let total = count_q.fetch_one(&self.pool).await?;

        // Build data query with bindings
        let mut data_q = sqlx::query(&data_sql);
        for v in &bind_values {
            data_q = data_q.bind(v);
        }
        let rows = data_q.fetch_all(&self.pool).await?;
        let data: Vec<ChannelRecord> = rows.iter().map(row_to_channel).collect::<DbResult<_>>()?;

        Ok(PaginatedChannels {
            data,
            total,
            page,
            page_size,
        })
    }

    async fn create(&self, input: CreateChannel) -> DbResult<ChannelRecord> {
        let status = if input.enabled { "active" } else { "disabled" };
        let mapping = input
            .model_mapping
            .unwrap_or(serde_json::Value::Object(Default::default()));
        let timeout = input.timeout_ms.unwrap_or(60000);
        let retries = input.max_retries.unwrap_or(2);
        let row = sqlx::query(
            "INSERT INTO channels (code, name, provider_type, base_url, supported_models, \
                                   config_enc, status, rpm_limit, tpm_limit, timeout_ms, max_retries, \
                                   tags, model_mapping) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
             RETURNING id, code, name, provider_type, base_url, supported_models, \
                       status, health, timeout_ms, max_retries, rpm_limit, tpm_limit, \
                       tags, model_mapping, balance, balance_updated_at, \
                       last_error, last_error_at, created_at, updated_at",
        )
        .bind(&input.code)
        .bind(&input.name)
        .bind(&input.provider_type)
        .bind(&input.base_url)
        .bind(&input.supported_models)
        .bind(b"" as &[u8])
        .bind(status)
        .bind(input.rpm_limit)
        .bind(input.tpm_limit)
        .bind(timeout)
        .bind(retries)
        .bind(&input.tags)
        .bind(&mapping)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db) if db.constraint().is_some() => {
                DbError::Conflict(format!("channel code '{}' already exists", input.code))
            }
            _ => DbError::from(e),
        })?;
        row_to_channel(&row)
    }

    async fn update(&self, id: ChannelId, input: UpdateChannel) -> DbResult<ChannelRecord> {
        let _ = self.find_by_id(id).await?;

        let status_param: Option<&str> = match input.enabled {
            Some(true) => Some("active"),
            Some(false) => Some("disabled"),
            None => None,
        };

        let row = sqlx::query(
            "UPDATE channels SET \
                name = COALESCE($2, name), \
                base_url = COALESCE($3, base_url), \
                supported_models = COALESCE($4, supported_models), \
                rpm_limit = CASE WHEN $5::INT IS NOT NULL THEN $5::INT ELSE rpm_limit END, \
                tpm_limit = CASE WHEN $6::INT IS NOT NULL THEN $6::INT ELSE tpm_limit END, \
                status = COALESCE($7, status), \
                timeout_ms = COALESCE($8, timeout_ms), \
                max_retries = COALESCE($9, max_retries), \
                tags = COALESCE($10, tags), \
                model_mapping = COALESCE($11, model_mapping), \
                updated_at = now() \
             WHERE id = $1 AND deleted_at IS NULL \
             RETURNING id, code, name, provider_type, base_url, supported_models, \
                       status, health, timeout_ms, max_retries, rpm_limit, tpm_limit, \
                       tags, model_mapping, balance, balance_updated_at, \
                       last_error, last_error_at, created_at, updated_at",
        )
        .bind(id.as_uuid())
        .bind(input.name.as_deref())
        .bind(input.base_url.as_deref())
        .bind(input.supported_models.as_deref())
        .bind(input.rpm_limit)
        .bind(input.tpm_limit)
        .bind(status_param)
        .bind(input.timeout_ms)
        .bind(input.max_retries)
        .bind(input.tags.as_deref())
        .bind(input.model_mapping.as_ref())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_channel(&row)
    }

    async fn set_status(&self, id: ChannelId, status: ChannelStatus) -> DbResult<ChannelRecord> {
        let row = sqlx::query(
            "UPDATE channels SET status = $2, updated_at = now() \
             WHERE id = $1 AND deleted_at IS NULL \
             RETURNING id, code, name, provider_type, base_url, supported_models, \
                       status, health, timeout_ms, max_retries, rpm_limit, tpm_limit, \
                       tags, model_mapping, balance, balance_updated_at, \
                       last_error, last_error_at, created_at, updated_at",
        )
        .bind(id.as_uuid())
        .bind(status.as_str())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_channel(&row)
    }

    async fn soft_delete(&self, id: ChannelId) -> DbResult<()> {
        let res = sqlx::query(
            "UPDATE channels SET deleted_at = NOW(), status = 'disabled' \
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    async fn auto_disable(&self, id: ChannelId, reason: &str) -> DbResult<()> {
        sqlx::query(
            "UPDATE channels SET status = 'disabled', health = 'unhealthy', \
             last_error = $2, last_error_at = NOW() \
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id.as_uuid())
        .bind(reason)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn re_enable(&self, id: ChannelId) -> DbResult<()> {
        sqlx::query(
            "UPDATE channels SET status = 'active', health = 'healthy', \
             last_error = NULL, last_error_at = NULL \
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn batch_set_enabled(&self, ids: &[ChannelId], enabled: bool) -> DbResult<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let uuids: Vec<Uuid> = ids.iter().map(|id| *id.as_uuid()).collect();
        let status = if enabled { "active" } else { "disabled" };
        let res = sqlx::query(
            "UPDATE channels SET status = $2, updated_at = now() \
             WHERE id = ANY($1) AND deleted_at IS NULL",
        )
        .bind(&uuids)
        .bind(status)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    async fn batch_soft_delete(&self, ids: &[ChannelId]) -> DbResult<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let uuids: Vec<Uuid> = ids.iter().map(|id| *id.as_uuid()).collect();
        let res = sqlx::query(
            "UPDATE channels SET deleted_at = NOW(), status = 'disabled', updated_at = now() \
             WHERE id = ANY($1) AND deleted_at IS NULL",
        )
        .bind(&uuids)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    async fn sync_models(&self, id: ChannelId, models: &[String]) -> DbResult<bool> {
        if models.is_empty() {
            return Ok(false);
        }
        let mut sorted = models.to_vec();
        sorted.sort();
        sorted.dedup();

        let res = sqlx::query(
            "UPDATE channels SET supported_models = $2, updated_at = NOW() \
             WHERE id = $1 AND deleted_at IS NULL \
             AND supported_models IS DISTINCT FROM $2",
        )
        .bind(id.as_uuid())
        .bind(&sorted)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }
}

// ============================================================================
// ChannelGroup
// ============================================================================

/// 渠道分组快照。
#[derive(Debug, Clone)]
pub struct ChannelGroupRecord {
    pub group_id: ChannelGroupId,
    pub name: String,
    pub description: String,
    pub strategy: String,
    pub fallback_group_id: Option<ChannelGroupId>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait ChannelGroupRepo: Send + Sync + 'static {
    async fn find_by_id(&self, id: ChannelGroupId) -> DbResult<ChannelGroupRecord>;
    async fn find_default_for_project(&self, project_id: ProjectId)
    -> DbResult<ChannelGroupRecord>;
    async fn list_all(&self) -> DbResult<Vec<ChannelGroupRecord>>;
    async fn create(&self, name: &str, strategy: &str) -> DbResult<ChannelGroupRecord>;
    /// Update group fields. `fallback_group_id`: outer None = don't change, Some(None) = clear, Some(Some(id)) = set.
    async fn update(
        &self,
        id: ChannelGroupId,
        name: Option<&str>,
        strategy: Option<&str>,
        enabled: Option<bool>,
        fallback_group_id: Option<Option<ChannelGroupId>>,
        description: Option<&str>,
    ) -> DbResult<ChannelGroupRecord>;
    async fn delete(&self, id: ChannelGroupId) -> DbResult<()>;
    async fn list_bindings(&self, group_id: ChannelGroupId) -> DbResult<Vec<ChannelBinding>>;
    async fn add_binding(
        &self,
        group_id: ChannelGroupId,
        channel_id: ChannelId,
        priority: i32,
        weight: i32,
    ) -> DbResult<()>;
    async fn remove_binding(&self, group_id: ChannelGroupId, channel_id: ChannelId)
    -> DbResult<()>;
    /// Update binding fields (all optional — None means keep current).
    async fn update_binding(
        &self,
        group_id: ChannelGroupId,
        channel_id: ChannelId,
        priority: Option<i32>,
        weight: Option<i32>,
        model_filter: Option<Vec<String>>,
        enabled: Option<bool>,
    ) -> DbResult<()>;
    /// List projects whose `default_group_id` references this group.
    async fn list_projects_using_group(&self, group_id: ChannelGroupId)
    -> DbResult<Vec<ProjectId>>;
    /// Set a project's default_group_id.
    async fn set_project_default_group(
        &self,
        project_id: ProjectId,
        group_id: Option<ChannelGroupId>,
    ) -> DbResult<()>;
}

pub struct PgChannelGroupRepo {
    pool: PgPool,
}

impl PgChannelGroupRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_group(row: &sqlx::postgres::PgRow) -> DbResult<ChannelGroupRecord> {
    let id: Uuid = row.try_get("id")?;
    let fallback: Option<Uuid> = row.try_get("fallback_group_id")?;
    Ok(ChannelGroupRecord {
        group_id: ChannelGroupId::from(id),
        name: row.try_get("name")?,
        description: row
            .try_get::<Option<String>, _>("description")?
            .unwrap_or_default(),
        strategy: row.try_get("strategy")?,
        fallback_group_id: fallback.map(ChannelGroupId::from),
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait]
impl ChannelGroupRepo for PgChannelGroupRepo {
    async fn find_by_id(&self, id: ChannelGroupId) -> DbResult<ChannelGroupRecord> {
        let row = sqlx::query(
            "SELECT id, name, description, strategy, fallback_group_id, enabled, created_at, updated_at \
             FROM channel_groups WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_group(&row)
    }

    async fn find_default_for_project(
        &self,
        project_id: ProjectId,
    ) -> DbResult<ChannelGroupRecord> {
        let row = sqlx::query(
            "SELECT cg.id, cg.name, cg.description, cg.strategy, cg.fallback_group_id, cg.enabled, \
                    cg.created_at, cg.updated_at \
             FROM projects p \
             JOIN channel_groups cg ON cg.id = p.default_group_id \
             WHERE p.id = $1",
        )
        .bind(project_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_group(&row)
    }

    async fn list_all(&self) -> DbResult<Vec<ChannelGroupRecord>> {
        let rows = sqlx::query(
            "SELECT id, name, description, strategy, fallback_group_id, enabled, created_at, updated_at \
             FROM channel_groups ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_group).collect()
    }

    async fn create(&self, name: &str, strategy: &str) -> DbResult<ChannelGroupRecord> {
        let row = sqlx::query(
            "INSERT INTO channel_groups (name, strategy) VALUES ($1, $2) \
             RETURNING id, name, description, strategy, fallback_group_id, enabled, created_at, updated_at",
        )
        .bind(name)
        .bind(strategy)
        .fetch_one(&self.pool)
        .await?;
        row_to_group(&row)
    }

    async fn update(
        &self,
        id: ChannelGroupId,
        name: Option<&str>,
        strategy: Option<&str>,
        enabled: Option<bool>,
        fallback_group_id: Option<Option<ChannelGroupId>>,
        description: Option<&str>,
    ) -> DbResult<ChannelGroupRecord> {
        // $6 = boolean flag: true means "apply fallback change", false means "keep current"
        // $7 = the new fallback_group_id (may be NULL to clear)
        let change_fallback = fallback_group_id.is_some();
        let new_fallback: Option<Uuid> = fallback_group_id.flatten().map(|gid| *gid.as_uuid());

        let row = sqlx::query(
            "UPDATE channel_groups SET \
             name = COALESCE($2, name), \
             strategy = COALESCE($3, strategy), \
             enabled = COALESCE($4, enabled), \
             description = COALESCE($5, description), \
             fallback_group_id = CASE WHEN $6::boolean THEN $7 ELSE fallback_group_id END, \
             updated_at = now() \
             WHERE id = $1 \
             RETURNING id, name, description, strategy, fallback_group_id, enabled, created_at, updated_at",
        )
        .bind(id.as_uuid())
        .bind(name)
        .bind(strategy)
        .bind(enabled)
        .bind(description)
        .bind(change_fallback)
        .bind(new_fallback)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_group(&row)
    }

    async fn delete(&self, id: ChannelGroupId) -> DbResult<()> {
        let res = sqlx::query("DELETE FROM channel_groups WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    async fn list_bindings(&self, group_id: ChannelGroupId) -> DbResult<Vec<ChannelBinding>> {
        let rows = sqlx::query(
            "SELECT c.id, c.code, c.name, c.provider_type, c.base_url, c.supported_models, \
                    c.status, c.health, c.timeout_ms, c.max_retries, c.rpm_limit, c.tpm_limit, \
                    c.tags, c.model_mapping, c.balance, c.balance_updated_at, \
                    c.last_error, c.last_error_at, c.created_at, c.updated_at, \
                    b.priority, b.weight, b.model_filter, b.enabled \
             FROM channel_group_bindings b \
             JOIN channels c ON c.id = b.channel_id \
             WHERE b.group_id = $1 AND c.deleted_at IS NULL \
             ORDER BY b.priority ASC",
        )
        .bind(group_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(ChannelBinding {
                    channel: row_to_channel(r)?,
                    priority: r.try_get("priority")?,
                    weight: r.try_get("weight")?,
                    model_filter: r.try_get("model_filter").unwrap_or_default(),
                    enabled: r.try_get("enabled").unwrap_or(true),
                })
            })
            .collect()
    }

    async fn add_binding(
        &self,
        group_id: ChannelGroupId,
        channel_id: ChannelId,
        priority: i32,
        weight: i32,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO channel_group_bindings (group_id, channel_id, priority, weight) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (group_id, channel_id) DO UPDATE SET priority = $3, weight = $4",
        )
        .bind(group_id.as_uuid())
        .bind(channel_id.as_uuid())
        .bind(priority)
        .bind(weight)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn remove_binding(
        &self,
        group_id: ChannelGroupId,
        channel_id: ChannelId,
    ) -> DbResult<()> {
        let res = sqlx::query(
            "DELETE FROM channel_group_bindings WHERE group_id = $1 AND channel_id = $2",
        )
        .bind(group_id.as_uuid())
        .bind(channel_id.as_uuid())
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    async fn update_binding(
        &self,
        group_id: ChannelGroupId,
        channel_id: ChannelId,
        priority: Option<i32>,
        weight: Option<i32>,
        model_filter: Option<Vec<String>>,
        enabled: Option<bool>,
    ) -> DbResult<()> {
        let res = sqlx::query(
            "UPDATE channel_group_bindings SET \
             priority = COALESCE($3, priority), \
             weight = COALESCE($4, weight), \
             model_filter = COALESCE($5, model_filter), \
             enabled = COALESCE($6, enabled) \
             WHERE group_id = $1 AND channel_id = $2",
        )
        .bind(group_id.as_uuid())
        .bind(channel_id.as_uuid())
        .bind(priority)
        .bind(weight)
        .bind(model_filter.as_deref())
        .bind(enabled)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    async fn list_projects_using_group(
        &self,
        group_id: ChannelGroupId,
    ) -> DbResult<Vec<ProjectId>> {
        let rows = sqlx::query("SELECT id FROM projects WHERE default_group_id = $1")
            .bind(group_id.as_uuid())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| {
                let id: Uuid = r.try_get("id").unwrap();
                ProjectId::from(id)
            })
            .collect())
    }

    async fn set_project_default_group(
        &self,
        project_id: ProjectId,
        group_id: Option<ChannelGroupId>,
    ) -> DbResult<()> {
        let gid: Option<Uuid> = group_id.map(|g| *g.as_uuid());
        let res = sqlx::query(
            "UPDATE projects SET default_group_id = $1, updated_at = now() \
             WHERE id = $2 AND deleted_at IS NULL",
        )
        .bind(gid)
        .bind(project_id.as_uuid())
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}

// ============================================================================
// InMemory 版（测试 / dev 模式）
// ============================================================================

use parking_lot::RwLock;
use std::collections::HashMap;

#[derive(Default)]
pub struct InMemoryChannelRepo {
    inner: RwLock<ChannelsInner>,
}

#[derive(Default)]
struct ChannelsInner {
    channels: HashMap<ChannelId, ChannelRecord>,
    /// group_id → Vec<(ChannelId, priority, weight)>
    bindings: HashMap<ChannelGroupId, Vec<(ChannelId, i32, i32)>>,
}

impl InMemoryChannelRepo {
    pub fn new() -> Self {
        Self::default()
    }

    /// 直接 seed 一条渠道记录。
    pub fn seed_channel(&self, record: ChannelRecord) {
        self.inner
            .write()
            .channels
            .insert(record.channel_id, record);
    }

    /// 建立 group → channel 绑定。
    pub fn seed_binding(
        &self,
        group_id: ChannelGroupId,
        channel_id: ChannelId,
        priority: i32,
        weight: i32,
    ) {
        self.inner
            .write()
            .bindings
            .entry(group_id)
            .or_default()
            .push((channel_id, priority, weight));
    }
}

#[async_trait]
impl ChannelRepo for InMemoryChannelRepo {
    async fn find_by_id(&self, id: ChannelId) -> DbResult<ChannelRecord> {
        self.inner
            .read()
            .channels
            .get(&id)
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn list_healthy_in_group(
        &self,
        group_id: ChannelGroupId,
    ) -> DbResult<Vec<ChannelBinding>> {
        let inner = self.inner.read();
        let bindings = match inner.bindings.get(&group_id) {
            Some(b) => b.clone(),
            None => return Ok(vec![]),
        };

        let mut result: Vec<ChannelBinding> = bindings
            .iter()
            .filter_map(|(ch_id, priority, weight)| {
                inner.channels.get(ch_id).and_then(|ch| {
                    if ch.is_healthy() {
                        Some(ChannelBinding {
                            channel: ch.clone(),
                            priority: *priority,
                            weight: *weight,
                            model_filter: vec![],
                            enabled: true,
                        })
                    } else {
                        None
                    }
                })
            })
            .collect();

        result.sort_by_key(|b| b.priority);
        Ok(result)
    }

    async fn list_admin_view(&self) -> DbResult<Vec<ChannelRecord>> {
        let inner = self.inner.read();
        let mut out: Vec<ChannelRecord> = inner.channels.values().cloned().collect();
        out.sort_by_key(|c| c.created_at);
        Ok(out)
    }

    async fn list_admin_paginated(&self, q: ListChannelsQuery) -> DbResult<PaginatedChannels> {
        let inner = self.inner.read();
        let mut out: Vec<ChannelRecord> = inner.channels.values().cloned().collect();

        if let Some(ref s) = q.search
            && !s.is_empty()
        {
            let s = s.to_lowercase();
            out.retain(|c| {
                c.code.to_lowercase().contains(&s) || c.name.to_lowercase().contains(&s)
            });
        }
        if let Some(ref p) = q.provider
            && !p.is_empty()
        {
            out.retain(|c| c.provider_type == *p);
        }
        if let Some(ref s) = q.status
            && !s.is_empty()
        {
            out.retain(|c| c.status == *s);
        }
        if let Some(ref h) = q.health
            && !h.is_empty()
        {
            out.retain(|c| c.health == *h);
        }

        let total = out.len() as i64;
        let page = q.page.max(1);
        let page_size = q.page_size.clamp(1, 100);
        let offset = ((page - 1) * page_size) as usize;
        out.sort_by_key(|c| c.created_at);
        let data: Vec<ChannelRecord> = out
            .into_iter()
            .skip(offset)
            .take(page_size as usize)
            .collect();
        Ok(PaginatedChannels {
            data,
            total,
            page,
            page_size,
        })
    }

    async fn create(&self, input: CreateChannel) -> DbResult<ChannelRecord> {
        let mut inner = self.inner.write();
        // 检查 code 唯一
        if inner.channels.values().any(|c| c.code == input.code) {
            return Err(DbError::Conflict(format!(
                "channel code '{}' already exists",
                input.code
            )));
        }
        let now = Utc::now();
        let id = ChannelId::from(Uuid::now_v7());
        let record = ChannelRecord {
            channel_id: id,
            code: input.code,
            name: input.name,
            provider_type: input.provider_type,
            base_url: input.base_url,
            supported_models: input.supported_models,
            status: if input.enabled {
                "active".to_string()
            } else {
                "disabled".to_string()
            },
            health: "healthy".to_string(),
            timeout_ms: input.timeout_ms.unwrap_or(60000),
            max_retries: input.max_retries.unwrap_or(2),
            rpm_limit: input.rpm_limit,
            tpm_limit: input.tpm_limit,
            tags: input.tags,
            model_mapping: input
                .model_mapping
                .unwrap_or(serde_json::Value::Object(Default::default())),
            balance: None,
            balance_updated_at: None,
            last_error: None,
            last_error_at: None,
            created_at: now,
            updated_at: now,
        };
        inner.channels.insert(id, record.clone());
        Ok(record)
    }

    async fn update(&self, id: ChannelId, input: UpdateChannel) -> DbResult<ChannelRecord> {
        let mut inner = self.inner.write();
        let record = inner.channels.get_mut(&id).ok_or(DbError::NotFound)?;
        if let Some(name) = input.name {
            record.name = name;
        }
        if let Some(base_url) = input.base_url {
            record.base_url = base_url;
        }
        if let Some(models) = input.supported_models {
            record.supported_models = models;
        }
        if let Some(enabled) = input.enabled {
            record.status = if enabled {
                "active".to_string()
            } else {
                "disabled".to_string()
            };
        }
        if let Some(v) = input.rpm_limit {
            record.rpm_limit = Some(v);
        }
        if let Some(v) = input.tpm_limit {
            record.tpm_limit = Some(v);
        }
        if let Some(v) = input.timeout_ms {
            record.timeout_ms = v;
        }
        if let Some(v) = input.max_retries {
            record.max_retries = v;
        }
        if let Some(tags) = input.tags {
            record.tags = tags;
        }
        if let Some(mapping) = input.model_mapping {
            record.model_mapping = mapping;
        }
        record.updated_at = Utc::now();
        Ok(record.clone())
    }

    async fn set_status(&self, id: ChannelId, status: ChannelStatus) -> DbResult<ChannelRecord> {
        let mut inner = self.inner.write();
        let record = inner.channels.get_mut(&id).ok_or(DbError::NotFound)?;
        record.status = status.as_str().to_string();
        record.updated_at = Utc::now();
        Ok(record.clone())
    }

    async fn soft_delete(&self, id: ChannelId) -> DbResult<()> {
        let mut inner = self.inner.write();
        if inner.channels.remove(&id).is_none() {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    async fn auto_disable(&self, id: ChannelId, reason: &str) -> DbResult<()> {
        let mut inner = self.inner.write();
        let record = inner.channels.get_mut(&id).ok_or(DbError::NotFound)?;
        record.status = "disabled".to_string();
        record.health = "unhealthy".to_string();
        let _ = reason; // stored conceptually; ChannelRecord has no last_error field in memory
        record.updated_at = Utc::now();
        Ok(())
    }

    async fn re_enable(&self, id: ChannelId) -> DbResult<()> {
        let mut inner = self.inner.write();
        let record = inner.channels.get_mut(&id).ok_or(DbError::NotFound)?;
        record.status = "active".to_string();
        record.health = "healthy".to_string();
        record.updated_at = Utc::now();
        Ok(())
    }

    async fn batch_set_enabled(&self, ids: &[ChannelId], enabled: bool) -> DbResult<u64> {
        let mut inner = self.inner.write();
        let mut count = 0u64;
        for id in ids {
            if let Some(ch) = inner.channels.get_mut(id) {
                ch.status = if enabled {
                    "active".into()
                } else {
                    "disabled".into()
                };
                ch.updated_at = Utc::now();
                count += 1;
            }
        }
        Ok(count)
    }

    async fn batch_soft_delete(&self, ids: &[ChannelId]) -> DbResult<u64> {
        let mut inner = self.inner.write();
        let mut count = 0u64;
        for id in ids {
            if inner.channels.remove(id).is_some() {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn sync_models(&self, id: ChannelId, models: &[String]) -> DbResult<bool> {
        let mut inner = self.inner.write();
        if let Some(ch) = inner.channels.get_mut(&id) {
            let mut sorted = models.to_vec();
            sorted.sort();
            sorted.dedup();
            if ch.supported_models != sorted {
                ch.supported_models = sorted;
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[derive(Default)]
pub struct InMemoryChannelGroupRepo {
    inner: RwLock<GroupsInner>,
}

#[derive(Default)]
struct GroupsInner {
    groups: HashMap<ChannelGroupId, ChannelGroupRecord>,
    /// project_id → default group_id
    defaults: HashMap<ProjectId, ChannelGroupId>,
}

impl InMemoryChannelGroupRepo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed_group(&self, record: ChannelGroupRecord) {
        self.inner.write().groups.insert(record.group_id, record);
    }

    pub fn seed_default(&self, project_id: ProjectId, group_id: ChannelGroupId) {
        self.inner.write().defaults.insert(project_id, group_id);
    }
}

#[async_trait]
impl ChannelGroupRepo for InMemoryChannelGroupRepo {
    async fn find_by_id(&self, id: ChannelGroupId) -> DbResult<ChannelGroupRecord> {
        self.inner
            .read()
            .groups
            .get(&id)
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn find_default_for_project(
        &self,
        project_id: ProjectId,
    ) -> DbResult<ChannelGroupRecord> {
        let inner = self.inner.read();
        let group_id = inner.defaults.get(&project_id).ok_or(DbError::NotFound)?;
        inner.groups.get(group_id).cloned().ok_or(DbError::NotFound)
    }

    async fn list_all(&self) -> DbResult<Vec<ChannelGroupRecord>> {
        Ok(self.inner.read().groups.values().cloned().collect())
    }

    async fn create(&self, name: &str, strategy: &str) -> DbResult<ChannelGroupRecord> {
        let now = Utc::now();
        let id = ChannelGroupId::from(Uuid::now_v7());
        let rec = ChannelGroupRecord {
            group_id: id,
            name: name.to_string(),
            description: String::new(),
            strategy: strategy.to_string(),
            fallback_group_id: None,
            enabled: true,
            created_at: now,
            updated_at: now,
        };
        self.inner.write().groups.insert(id, rec.clone());
        Ok(rec)
    }

    async fn update(
        &self,
        id: ChannelGroupId,
        name: Option<&str>,
        strategy: Option<&str>,
        enabled: Option<bool>,
        fallback_group_id: Option<Option<ChannelGroupId>>,
        description: Option<&str>,
    ) -> DbResult<ChannelGroupRecord> {
        let mut inner = self.inner.write();
        let g = inner.groups.get_mut(&id).ok_or(DbError::NotFound)?;
        if let Some(n) = name {
            g.name = n.to_string();
        }
        if let Some(s) = strategy {
            g.strategy = s.to_string();
        }
        if let Some(e) = enabled {
            g.enabled = e;
        }
        if let Some(d) = description {
            g.description = d.to_string();
        }
        if let Some(fb) = fallback_group_id {
            g.fallback_group_id = fb;
        }
        g.updated_at = Utc::now();
        Ok(g.clone())
    }

    async fn delete(&self, id: ChannelGroupId) -> DbResult<()> {
        if self.inner.write().groups.remove(&id).is_none() {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    async fn list_bindings(&self, _group_id: ChannelGroupId) -> DbResult<Vec<ChannelBinding>> {
        Ok(vec![])
    }

    async fn add_binding(
        &self,
        _group_id: ChannelGroupId,
        _channel_id: ChannelId,
        _priority: i32,
        _weight: i32,
    ) -> DbResult<()> {
        Ok(())
    }

    async fn remove_binding(
        &self,
        _group_id: ChannelGroupId,
        _channel_id: ChannelId,
    ) -> DbResult<()> {
        Ok(())
    }

    async fn update_binding(
        &self,
        _group_id: ChannelGroupId,
        _channel_id: ChannelId,
        _priority: Option<i32>,
        _weight: Option<i32>,
        _model_filter: Option<Vec<String>>,
        _enabled: Option<bool>,
    ) -> DbResult<()> {
        Ok(())
    }

    async fn list_projects_using_group(
        &self,
        group_id: ChannelGroupId,
    ) -> DbResult<Vec<ProjectId>> {
        let inner = self.inner.read();
        Ok(inner
            .defaults
            .iter()
            .filter(|(_, gid)| **gid == group_id)
            .map(|(pid, _)| *pid)
            .collect())
    }

    async fn set_project_default_group(
        &self,
        project_id: ProjectId,
        group_id: Option<ChannelGroupId>,
    ) -> DbResult<()> {
        let mut inner = self.inner.write();
        match group_id {
            Some(gid) => {
                inner.defaults.insert(project_id, gid);
            }
            None => {
                inner.defaults.remove(&project_id);
            }
        }
        Ok(())
    }
}
