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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
}

/// 更新 Channel 的入参（全部可选，None 表示不改）。
#[derive(Debug, Clone)]
pub struct UpdateChannel {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub supported_models: Option<Vec<String>>,
    pub enabled: Option<bool>,
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

    /// 创建新 channel（admin 操作）。
    async fn create(&self, input: CreateChannel) -> DbResult<ChannelRecord>;

    /// 更新 channel 字段（admin 操作）。
    async fn update(&self, id: ChannelId, input: UpdateChannel) -> DbResult<ChannelRecord>;

    /// 软删除（设 deleted_at + status='disabled'）。
    async fn soft_delete(&self, id: ChannelId) -> DbResult<()>;
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
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait]
impl ChannelRepo for PgChannelRepo {
    async fn find_by_id(&self, id: ChannelId) -> DbResult<ChannelRecord> {
        let row = sqlx::query(
            "SELECT id, code, name, provider_type, base_url, supported_models, \
                    status, health, timeout_ms, max_retries, created_at, updated_at \
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
                    c.status, c.health, c.timeout_ms, c.max_retries, c.created_at, c.updated_at, \
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
                })
            })
            .collect()
    }

    async fn list_admin_view(&self) -> DbResult<Vec<ChannelRecord>> {
        let rows = sqlx::query(
            "SELECT id, code, name, provider_type, base_url, supported_models, \
                    status, health, timeout_ms, max_retries, created_at, updated_at \
             FROM channels WHERE deleted_at IS NULL \
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_channel).collect()
    }

    async fn create(&self, input: CreateChannel) -> DbResult<ChannelRecord> {
        let status = if input.enabled { "active" } else { "disabled" };
        let row = sqlx::query(
            "INSERT INTO channels (code, name, provider_type, base_url, supported_models, \
                                   config_enc, status) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING id, code, name, provider_type, base_url, supported_models, \
                       status, health, timeout_ms, max_retries, created_at, updated_at",
        )
        .bind(&input.code)
        .bind(&input.name)
        .bind(&input.provider_type)
        .bind(&input.base_url)
        .bind(&input.supported_models)
        .bind(b"" as &[u8]) // config_enc placeholder — 后续加密配置另走
        .bind(status)
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
        // 先确认存在
        let _ = self.find_by_id(id).await?;

        let status_fragment = match input.enabled {
            Some(true) => ", status = 'active'",
            Some(false) => ", status = 'disabled'",
            None => "",
        };

        let row = sqlx::query(&format!(
            "UPDATE channels SET \
                name = COALESCE($2, name), \
                base_url = COALESCE($3, base_url), \
                supported_models = COALESCE($4, supported_models) \
                {status_fragment} \
             WHERE id = $1 AND deleted_at IS NULL \
             RETURNING id, code, name, provider_type, base_url, supported_models, \
                       status, health, timeout_ms, max_retries, created_at, updated_at"
        ))
        .bind(id.as_uuid())
        .bind(input.name.as_deref())
        .bind(input.base_url.as_deref())
        .bind(input.supported_models.as_deref())
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
}

// ============================================================================
// ChannelGroup
// ============================================================================

/// 渠道分组快照。
#[derive(Debug, Clone)]
pub struct ChannelGroupRecord {
    pub group_id: ChannelGroupId,
    pub name: String,
    pub strategy: String,
    pub fallback_group_id: Option<ChannelGroupId>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait ChannelGroupRepo: Send + Sync + 'static {
    async fn find_by_id(&self, id: ChannelGroupId) -> DbResult<ChannelGroupRecord>;
    async fn find_default_for_project(&self, project_id: ProjectId) -> DbResult<ChannelGroupRecord>;
    async fn list_all(&self) -> DbResult<Vec<ChannelGroupRecord>>;
    async fn create(&self, name: &str, strategy: &str) -> DbResult<ChannelGroupRecord>;
    async fn update(&self, id: ChannelGroupId, name: Option<&str>, strategy: Option<&str>, enabled: Option<bool>) -> DbResult<ChannelGroupRecord>;
    async fn delete(&self, id: ChannelGroupId) -> DbResult<()>;
    async fn list_bindings(&self, group_id: ChannelGroupId) -> DbResult<Vec<ChannelBinding>>;
    async fn add_binding(&self, group_id: ChannelGroupId, channel_id: ChannelId, priority: i32, weight: i32) -> DbResult<()>;
    async fn remove_binding(&self, group_id: ChannelGroupId, channel_id: ChannelId) -> DbResult<()>;
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
            "SELECT id, name, strategy, fallback_group_id, enabled, created_at, updated_at \
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
            "SELECT cg.id, cg.name, cg.strategy, cg.fallback_group_id, cg.enabled, \
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
            "SELECT id, name, strategy, fallback_group_id, enabled, created_at, updated_at \
             FROM channel_groups ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_group).collect()
    }

    async fn create(&self, name: &str, strategy: &str) -> DbResult<ChannelGroupRecord> {
        let row = sqlx::query(
            "INSERT INTO channel_groups (name, strategy) VALUES ($1, $2) \
             RETURNING id, name, strategy, fallback_group_id, enabled, created_at, updated_at",
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
    ) -> DbResult<ChannelGroupRecord> {
        let row = sqlx::query(
            "UPDATE channel_groups SET \
             name = COALESCE($2, name), \
             strategy = COALESCE($3, strategy), \
             enabled = COALESCE($4, enabled), \
             updated_at = now() \
             WHERE id = $1 \
             RETURNING id, name, strategy, fallback_group_id, enabled, created_at, updated_at",
        )
        .bind(id.as_uuid())
        .bind(name)
        .bind(strategy)
        .bind(enabled)
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
        if res.rows_affected() == 0 { return Err(DbError::NotFound); }
        Ok(())
    }

    async fn list_bindings(&self, group_id: ChannelGroupId) -> DbResult<Vec<ChannelBinding>> {
        let rows = sqlx::query(
            "SELECT c.id, c.code, c.name, c.provider_type, c.base_url, c.supported_models, \
                    c.status, c.health, c.timeout_ms, c.max_retries, c.created_at, c.updated_at, \
                    b.priority, b.weight, b.model_filter \
             FROM channel_group_bindings b \
             JOIN channels c ON c.id = b.channel_id \
             WHERE b.group_id = $1 AND c.deleted_at IS NULL \
             ORDER BY b.priority ASC",
        )
        .bind(group_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| Ok(ChannelBinding {
                channel: row_to_channel(r)?,
                priority: r.try_get("priority")?,
                weight: r.try_get("weight")?,
                model_filter: r.try_get("model_filter").unwrap_or_default(),
            }))
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
        if res.rows_affected() == 0 { return Err(DbError::NotFound); }
        Ok(())
    }
}

// ============================================================================
// InMemory 版（测试 / dev 模式）
// ============================================================================

use std::collections::HashMap;
use std::sync::RwLock;

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
            .unwrap()
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
            .unwrap()
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
            .unwrap()
            .channels
            .get(&id)
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn list_healthy_in_group(
        &self,
        group_id: ChannelGroupId,
    ) -> DbResult<Vec<ChannelBinding>> {
        let inner = self.inner.read().unwrap();
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
        let inner = self.inner.read().unwrap();
        let mut out: Vec<ChannelRecord> = inner.channels.values().cloned().collect();
        out.sort_by_key(|c| c.created_at);
        Ok(out)
    }

    async fn create(&self, input: CreateChannel) -> DbResult<ChannelRecord> {
        let mut inner = self.inner.write().unwrap();
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
            timeout_ms: 60000,
            max_retries: 2,
            created_at: now,
            updated_at: now,
        };
        inner.channels.insert(id, record.clone());
        Ok(record)
    }

    async fn update(&self, id: ChannelId, input: UpdateChannel) -> DbResult<ChannelRecord> {
        let mut inner = self.inner.write().unwrap();
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
        record.updated_at = Utc::now();
        Ok(record.clone())
    }

    async fn soft_delete(&self, id: ChannelId) -> DbResult<()> {
        let mut inner = self.inner.write().unwrap();
        if inner.channels.remove(&id).is_none() {
            return Err(DbError::NotFound);
        }
        Ok(())
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
        self.inner
            .write()
            .unwrap()
            .groups
            .insert(record.group_id, record);
    }

    pub fn seed_default(&self, project_id: ProjectId, group_id: ChannelGroupId) {
        self.inner
            .write()
            .unwrap()
            .defaults
            .insert(project_id, group_id);
    }
}

#[async_trait]
impl ChannelGroupRepo for InMemoryChannelGroupRepo {
    async fn find_by_id(&self, id: ChannelGroupId) -> DbResult<ChannelGroupRecord> {
        self.inner
            .read()
            .unwrap()
            .groups
            .get(&id)
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn find_default_for_project(
        &self,
        project_id: ProjectId,
    ) -> DbResult<ChannelGroupRecord> {
        let inner = self.inner.read().unwrap();
        let group_id = inner.defaults.get(&project_id).ok_or(DbError::NotFound)?;
        inner.groups.get(group_id).cloned().ok_or(DbError::NotFound)
    }

    async fn list_all(&self) -> DbResult<Vec<ChannelGroupRecord>> {
        Ok(self.inner.read().unwrap().groups.values().cloned().collect())
    }

    async fn create(&self, name: &str, strategy: &str) -> DbResult<ChannelGroupRecord> {
        let now = Utc::now();
        let id = ChannelGroupId::from(Uuid::now_v7());
        let rec = ChannelGroupRecord {
            group_id: id, name: name.to_string(), strategy: strategy.to_string(),
            fallback_group_id: None, enabled: true, created_at: now, updated_at: now,
        };
        self.inner.write().unwrap().groups.insert(id, rec.clone());
        Ok(rec)
    }

    async fn update(&self, id: ChannelGroupId, name: Option<&str>, strategy: Option<&str>, enabled: Option<bool>) -> DbResult<ChannelGroupRecord> {
        let mut inner = self.inner.write().unwrap();
        let g = inner.groups.get_mut(&id).ok_or(DbError::NotFound)?;
        if let Some(n) = name { g.name = n.to_string(); }
        if let Some(s) = strategy { g.strategy = s.to_string(); }
        if let Some(e) = enabled { g.enabled = e; }
        g.updated_at = Utc::now();
        Ok(g.clone())
    }

    async fn delete(&self, id: ChannelGroupId) -> DbResult<()> {
        if self.inner.write().unwrap().groups.remove(&id).is_none() { return Err(DbError::NotFound); }
        Ok(())
    }

    async fn list_bindings(&self, _group_id: ChannelGroupId) -> DbResult<Vec<ChannelBinding>> {
        Ok(vec![])
    }

    async fn add_binding(&self, _group_id: ChannelGroupId, _channel_id: ChannelId, _priority: i32, _weight: i32) -> DbResult<()> {
        Ok(())
    }

    async fn remove_binding(&self, _group_id: ChannelGroupId, _channel_id: ChannelId) -> DbResult<()> {
        Ok(())
    }
}
