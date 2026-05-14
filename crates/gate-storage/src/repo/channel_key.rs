//! ChannelKeyRepo — 渠道 API Key 池（加密存储 + 轮转）。
//!
//! 每个 channel 可挂多个 key（支持 rotation），status 由 health 字段表达：
//! - `healthy`       = 可用
//! - `cooling_down`  = 熔断冷却中（由调用链路自动管理，不影响本 repo 逻辑）
//! - `disabled`      = 已停用/已撤销
//!
//! 选 key 策略：healthy 且非冷却期内，按 weight DESC + created_at ASC。

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gate_core::id::{ChannelId, ChannelKeyId};
use sqlx::{PgPool, Row};
use uuid::Uuid;

// ============================================================================
// Record
// ============================================================================

#[derive(Debug, Clone)]
pub struct ChannelKeyRecord {
    pub id: ChannelKeyId,
    pub channel_id: ChannelId,
    pub label: Option<String>,
    pub key_enc: Vec<u8>,
    pub key_fingerprint: String,
    pub weight: i32,
    pub health: String,
    pub consecutive_errors: i32,
    pub total_requests: i64,
    pub total_errors: i64,
    pub last_error_code: Option<i32>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ============================================================================
// Trait
// ============================================================================

#[async_trait]
pub trait ChannelKeyRepo: Send + Sync + 'static {
    /// 取该 channel 的「当前最佳可用 key」——健康、不在冷却期、按 weight 降序。
    async fn find_active_for_channel(
        &self,
        channel_id: ChannelId,
    ) -> DbResult<ChannelKeyRecord>;

    /// 列出某 channel 的全部 key（admin 视图，含 disabled）。
    async fn list_by_channel(
        &self,
        channel_id: ChannelId,
    ) -> DbResult<Vec<ChannelKeyRecord>>;

    /// 新增一把 key（加密后的 secret + fingerprint）。
    async fn create(
        &self,
        channel_id: ChannelId,
        key_enc: &[u8],
        key_fingerprint: &str,
        label: Option<&str>,
    ) -> DbResult<ChannelKeyId>;

    /// 轮转：disable 旧 key、插入新 key，原子操作。
    async fn rotate(
        &self,
        channel_id: ChannelId,
        new_key_enc: &[u8],
        new_fingerprint: &str,
        label: Option<&str>,
    ) -> DbResult<ChannelKeyId>;

    /// 撤销指定 key（设 health='disabled'）。
    async fn revoke(&self, key_id: ChannelKeyId) -> DbResult<()>;

    /// 记录一次成功调用：清零 consecutive_errors，health 置 healthy，total_requests +1。
    async fn report_success(&self, key_id: ChannelKeyId) -> DbResult<()>;

    /// 记录一次失败调用：consecutive_errors +1，total_errors +1，total_requests +1，
    /// 写入 last_error_code / last_error_at；达到阈值（≥3）后进入 cooling_down 并设 cooldown_until。
    async fn report_failure(
        &self,
        key_id: ChannelKeyId,
        error_code: Option<i32>,
        cooldown_secs: i64,
    ) -> DbResult<()>;
}

// ============================================================================
// Pg 实现
// ============================================================================

pub struct PgChannelKeyRepo {
    pool: PgPool,
}

impl PgChannelKeyRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_channel_key(row: &sqlx::postgres::PgRow) -> DbResult<ChannelKeyRecord> {
    let id: Uuid = row.try_get("id")?;
    let channel_id: Uuid = row.try_get("channel_id")?;
    Ok(ChannelKeyRecord {
        id: ChannelKeyId::from(id),
        channel_id: ChannelId::from(channel_id),
        label: row.try_get("label")?,
        key_enc: row.try_get("key_enc")?,
        key_fingerprint: row.try_get("key_fingerprint")?,
        weight: row.try_get("weight")?,
        health: row.try_get("health")?,
        consecutive_errors: row.try_get("consecutive_errors")?,
        total_requests: row.try_get("total_requests")?,
        total_errors: row.try_get("total_errors")?,
        last_error_code: row.try_get("last_error_code")?,
        last_error_at: row.try_get("last_error_at")?,
        cooldown_until: row.try_get("cooldown_until")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait]
impl ChannelKeyRepo for PgChannelKeyRepo {
    async fn find_active_for_channel(
        &self,
        channel_id: ChannelId,
    ) -> DbResult<ChannelKeyRecord> {
        let row = sqlx::query(
            "SELECT id, channel_id, label, key_enc, key_fingerprint, weight, \
                    health, consecutive_errors, total_requests, total_errors, \
                    last_error_code, last_error_at, cooldown_until, \
                    created_at, updated_at \
             FROM channel_keys \
             WHERE channel_id = $1 \
               AND health = 'healthy' \
               AND (cooldown_until IS NULL OR cooldown_until < NOW()) \
             ORDER BY weight DESC, created_at ASC \
             LIMIT 1",
        )
        .bind(channel_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_channel_key(&row)
    }

    async fn list_by_channel(
        &self,
        channel_id: ChannelId,
    ) -> DbResult<Vec<ChannelKeyRecord>> {
        let rows = sqlx::query(
            "SELECT id, channel_id, label, key_enc, key_fingerprint, weight, \
                    health, consecutive_errors, total_requests, total_errors, \
                    last_error_code, last_error_at, cooldown_until, \
                    created_at, updated_at \
             FROM channel_keys \
             WHERE channel_id = $1 \
             ORDER BY created_at DESC",
        )
        .bind(channel_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_channel_key).collect()
    }

    async fn create(
        &self,
        channel_id: ChannelId,
        key_enc: &[u8],
        key_fingerprint: &str,
        label: Option<&str>,
    ) -> DbResult<ChannelKeyId> {
        let row = sqlx::query(
            "INSERT INTO channel_keys (channel_id, key_enc, key_fingerprint, label) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id",
        )
        .bind(channel_id.as_uuid())
        .bind(key_enc)
        .bind(key_fingerprint)
        .bind(label)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db) if db.constraint().is_some() => {
                DbError::Conflict(
                    "duplicate key fingerprint for channel".to_string(),
                )
            }
            _ => DbError::from(e),
        })?;
        let id: Uuid = row.try_get("id")?;
        Ok(ChannelKeyId::from(id))
    }

    async fn rotate(
        &self,
        channel_id: ChannelId,
        new_key_enc: &[u8],
        new_fingerprint: &str,
        label: Option<&str>,
    ) -> DbResult<ChannelKeyId> {
        let mut tx = self.pool.begin().await.map_err(|e| DbError::Internal(e.to_string()))?;

        // disable 全部旧 healthy key
        sqlx::query(
            "UPDATE channel_keys SET health = 'disabled' \
             WHERE channel_id = $1 AND health = 'healthy'",
        )
        .bind(channel_id.as_uuid())
        .execute(&mut *tx)
        .await?;

        // 插入新 key
        let row = sqlx::query(
            "INSERT INTO channel_keys (channel_id, key_enc, key_fingerprint, label) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id",
        )
        .bind(channel_id.as_uuid())
        .bind(new_key_enc)
        .bind(new_fingerprint)
        .bind(label)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db) if db.constraint().is_some() => {
                DbError::Conflict(
                    "duplicate key fingerprint for channel".to_string(),
                )
            }
            _ => DbError::from(e),
        })?;

        tx.commit().await.map_err(|e| DbError::Internal(e.to_string()))?;
        let id: Uuid = row.try_get("id")?;
        Ok(ChannelKeyId::from(id))
    }

    async fn revoke(&self, key_id: ChannelKeyId) -> DbResult<()> {
        let res = sqlx::query(
            "UPDATE channel_keys SET health = 'disabled' WHERE id = $1",
        )
        .bind(key_id.as_uuid())
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    async fn report_success(&self, key_id: ChannelKeyId) -> DbResult<()> {
        sqlx::query(
            "UPDATE channel_keys SET \
             consecutive_errors = 0, \
             health = 'healthy', \
             total_requests = total_requests + 1, \
             cooldown_until = NULL \
             WHERE id = $1",
        )
        .bind(key_id.as_uuid())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn report_failure(
        &self,
        key_id: ChannelKeyId,
        error_code: Option<i32>,
        cooldown_secs: i64,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE channel_keys SET \
             consecutive_errors = consecutive_errors + 1, \
             total_requests = total_requests + 1, \
             total_errors = total_errors + 1, \
             last_error_code = $2, \
             last_error_at = NOW(), \
             health = CASE WHEN consecutive_errors + 1 >= 3 THEN 'cooling_down' ELSE health END, \
             cooldown_until = CASE WHEN consecutive_errors + 1 >= 3 \
                                   THEN NOW() + ($3 || ' seconds')::INTERVAL \
                                   ELSE cooldown_until END \
             WHERE id = $1",
        )
        .bind(key_id.as_uuid())
        .bind(error_code)
        .bind(cooldown_secs.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

// ============================================================================
// InMemory 版（测试 / dev）
// ============================================================================

use std::collections::HashMap;
use parking_lot::RwLock;

#[derive(Default)]
pub struct InMemoryChannelKeyRepo {
    inner: RwLock<KeysInner>,
}

#[derive(Default)]
struct KeysInner {
    keys: HashMap<ChannelKeyId, ChannelKeyRecord>,
}

impl InMemoryChannelKeyRepo {
    pub fn new() -> Self {
        Self::default()
    }

    /// 测试直接 seed 一条记录。
    pub fn seed(&self, record: ChannelKeyRecord) {
        self.inner.write().keys.insert(record.id, record);
    }
}

#[async_trait]
impl ChannelKeyRepo for InMemoryChannelKeyRepo {
    async fn find_active_for_channel(
        &self,
        channel_id: ChannelId,
    ) -> DbResult<ChannelKeyRecord> {
        let inner = self.inner.read();
        inner
            .keys
            .values()
            .filter(|k| {
                k.channel_id == channel_id
                    && k.health == "healthy"
            })
            .max_by_key(|k| (k.weight, std::cmp::Reverse(k.created_at)))
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn list_by_channel(
        &self,
        channel_id: ChannelId,
    ) -> DbResult<Vec<ChannelKeyRecord>> {
        let inner = self.inner.read();
        let mut out: Vec<_> = inner
            .keys
            .values()
            .filter(|k| k.channel_id == channel_id)
            .cloned()
            .collect();
        out.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        Ok(out)
    }

    async fn create(
        &self,
        channel_id: ChannelId,
        key_enc: &[u8],
        key_fingerprint: &str,
        label: Option<&str>,
    ) -> DbResult<ChannelKeyId> {
        let mut inner = self.inner.write();
        // 检查 fingerprint 唯一
        if inner
            .keys
            .values()
            .any(|k| k.channel_id == channel_id && k.key_fingerprint == key_fingerprint)
        {
            return Err(DbError::Conflict(
                "duplicate key fingerprint for channel".to_string(),
            ));
        }
        let now = Utc::now();
        let id = ChannelKeyId::from(Uuid::now_v7());
        let record = ChannelKeyRecord {
            id,
            channel_id,
            label: label.map(String::from),
            key_enc: key_enc.to_vec(),
            key_fingerprint: key_fingerprint.to_string(),
            weight: 1,
            health: "healthy".to_string(),
            consecutive_errors: 0,
            total_requests: 0,
            total_errors: 0,
            last_error_code: None,
            last_error_at: None,
            cooldown_until: None,
            created_at: now,
            updated_at: now,
        };
        inner.keys.insert(id, record);
        Ok(id)
    }

    async fn rotate(
        &self,
        channel_id: ChannelId,
        new_key_enc: &[u8],
        new_fingerprint: &str,
        label: Option<&str>,
    ) -> DbResult<ChannelKeyId> {
        let mut inner = self.inner.write();
        // disable 全部旧 healthy key
        for k in inner.keys.values_mut() {
            if k.channel_id == channel_id && k.health == "healthy" {
                k.health = "disabled".to_string();
                k.updated_at = Utc::now();
            }
        }
        // 检查 fingerprint 唯一
        if inner
            .keys
            .values()
            .any(|k| k.channel_id == channel_id && k.key_fingerprint == new_fingerprint)
        {
            return Err(DbError::Conflict(
                "duplicate key fingerprint for channel".to_string(),
            ));
        }
        let now = Utc::now();
        let id = ChannelKeyId::from(Uuid::now_v7());
        let record = ChannelKeyRecord {
            id,
            channel_id,
            label: label.map(String::from),
            key_enc: new_key_enc.to_vec(),
            key_fingerprint: new_fingerprint.to_string(),
            weight: 1,
            health: "healthy".to_string(),
            consecutive_errors: 0,
            total_requests: 0,
            total_errors: 0,
            last_error_code: None,
            last_error_at: None,
            cooldown_until: None,
            created_at: now,
            updated_at: now,
        };
        inner.keys.insert(id, record);
        Ok(id)
    }

    async fn revoke(&self, key_id: ChannelKeyId) -> DbResult<()> {
        let mut inner = self.inner.write();
        let rec = inner.keys.get_mut(&key_id).ok_or(DbError::NotFound)?;
        rec.health = "disabled".to_string();
        rec.updated_at = Utc::now();
        Ok(())
    }

    async fn report_success(&self, _key_id: ChannelKeyId) -> DbResult<()> {
        Ok(())
    }

    async fn report_failure(
        &self,
        _key_id: ChannelKeyId,
        _error_code: Option<i32>,
        _cooldown_secs: i64,
    ) -> DbResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_channel_id() -> ChannelId {
        ChannelId::from(Uuid::now_v7())
    }

    #[tokio::test]
    async fn inmemory_create_and_find() {
        let repo = InMemoryChannelKeyRepo::new();
        let ch = test_channel_id();
        let id = repo
            .create(ch, b"encrypted-key", "fp-abc", Some("primary"))
            .await
            .unwrap();

        let found = repo.find_active_for_channel(ch).await.unwrap();
        assert_eq!(found.id, id);
        assert_eq!(found.label.as_deref(), Some("primary"));
        assert_eq!(found.key_enc, b"encrypted-key");
        assert_eq!(found.health, "healthy");
    }

    #[tokio::test]
    async fn inmemory_list_by_channel() {
        let repo = InMemoryChannelKeyRepo::new();
        let ch = test_channel_id();
        let _ = repo.create(ch, b"k1", "fp-1", Some("a")).await.unwrap();
        let _ = repo.create(ch, b"k2", "fp-2", Some("b")).await.unwrap();

        let list = repo.list_by_channel(ch).await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn inmemory_revoke() {
        let repo = InMemoryChannelKeyRepo::new();
        let ch = test_channel_id();
        let id = repo.create(ch, b"k1", "fp-1", None).await.unwrap();
        repo.revoke(id).await.unwrap();

        // should not find active
        let res = repo.find_active_for_channel(ch).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn inmemory_rotate() {
        let repo = InMemoryChannelKeyRepo::new();
        let ch = test_channel_id();
        let old_id = repo.create(ch, b"k-old", "fp-old", Some("old")).await.unwrap();
        let new_id = repo
            .rotate(ch, b"k-new", "fp-new", Some("new"))
            .await
            .unwrap();

        // old key should be disabled
        let list = repo.list_by_channel(ch).await.unwrap();
        let old = list.iter().find(|k| k.id == old_id).unwrap();
        assert_eq!(old.health, "disabled");

        // new key should be active
        let active = repo.find_active_for_channel(ch).await.unwrap();
        assert_eq!(active.id, new_id);
        assert_eq!(active.label.as_deref(), Some("new"));
    }

    #[tokio::test]
    async fn inmemory_duplicate_fingerprint_rejected() {
        let repo = InMemoryChannelKeyRepo::new();
        let ch = test_channel_id();
        repo.create(ch, b"k1", "fp-same", None).await.unwrap();
        let res = repo.create(ch, b"k2", "fp-same", None).await;
        assert!(matches!(res, Err(DbError::Conflict(_))));
    }

    #[tokio::test]
    async fn inmemory_no_active_returns_not_found() {
        let repo = InMemoryChannelKeyRepo::new();
        let ch = test_channel_id();
        let res = repo.find_active_for_channel(ch).await;
        assert!(matches!(res, Err(DbError::NotFound)));
    }
}
