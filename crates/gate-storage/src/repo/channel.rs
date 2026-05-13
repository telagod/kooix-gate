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
                    b.priority, b.weight \
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
                })
            })
            .collect()
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
    /// 按 ID 查分组。
    async fn find_by_id(&self, id: ChannelGroupId) -> DbResult<ChannelGroupRecord>;

    /// 查 Project 的默认分组（通过 projects.default_group_id）。
    async fn find_default_for_project(&self, project_id: ProjectId)
    -> DbResult<ChannelGroupRecord>;
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
}
