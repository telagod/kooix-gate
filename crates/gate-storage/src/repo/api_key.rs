//! ApiKeyRepo — 按 hash 查/撤销/列表。
//!
//! 关键不变式：
//! - `key_hash` 唯一索引保证 O(1) 查；撤销走软删（`revoked_at`），不真删行。
//! - `allowed_ips` 是 CIDR[]，调用方自己判断 IP 是否命中（含/24 之类的网段）。
//! - 过期判断由调用方结合 `expires_at` 做，这里只提供数据。

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gate_core::id::{ApiKeyId, OrgId, ProjectId, UserId};
use sqlx::types::ipnetwork::IpNetwork;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// 存储层快照 — PgLoader 会据此构造 AuthContext::api_key + 做 IP/过期检查。
#[derive(Debug, Clone)]
pub struct ApiKeyRecord {
    pub api_key_id: ApiKeyId,
    pub project_id: ProjectId,
    pub org_id: OrgId,
    pub name: String,
    pub allowed_ips: Vec<IpNetwork>,
    pub allowed_models: Vec<String>,
    pub allowed_groups: Vec<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// 概要视图 — 列表展示用，含 prefix/last4 等展示字段。
#[derive(Debug, Clone)]
pub struct ApiKeySummaryRecord {
    pub api_key_id: ApiKeyId,
    pub name: String,
    pub prefix: String,
    pub last4: String,
    pub allowed_models: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl ApiKeyRecord {
    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|t| t <= now)
    }
}

#[async_trait]
pub trait ApiKeyRepo: Send + Sync + 'static {
    /// 通过 hash 查完整记录（用于认证热路径）。
    async fn find_by_hash(&self, hash: &str) -> DbResult<ApiKeyRecord>;

    /// 创建新 key；hash/prefix/last4 在 gate-auth 里已算好。
    #[allow(clippy::too_many_arguments)]
    async fn create(
        &self,
        project_id: ProjectId,
        name: &str,
        key_hash: &str,
        key_prefix: &str,
        key_last4: &str,
        created_by: UserId,
        allowed_models: &[String],
    ) -> DbResult<ApiKeyId>;

    async fn list_in_project(&self, project_id: ProjectId) -> DbResult<Vec<ApiKeyRecord>>;

    /// 概要列表（含 prefix/last4/created_at/last_used_at），控制台展示用。
    async fn list_summaries(&self, project_id: ProjectId) -> DbResult<Vec<ApiKeySummaryRecord>>;

    async fn revoke(&self, id: ApiKeyId, by: UserId, reason: Option<&str>) -> DbResult<()>;

    /// 热路径后置：记录最近使用。允许忽略错误（日志即可）。
    async fn touch_used(
        &self,
        id: ApiKeyId,
        at: DateTime<Utc>,
        ip: Option<std::net::IpAddr>,
    ) -> DbResult<()>;
}

pub struct PgApiKeyRepo {
    pool: PgPool,
}

impl PgApiKeyRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_record(row: &sqlx::postgres::PgRow) -> DbResult<ApiKeyRecord> {
    let id: Uuid = row.try_get("id")?;
    let project_id: Uuid = row.try_get("project_id")?;
    let org_id: Uuid = row.try_get("org_id")?;
    let allowed_ips: Vec<IpNetwork> = row.try_get("allowed_ips")?;
    let allowed_models: Vec<String> = row.try_get("allowed_models")?;
    let allowed_groups: Vec<Uuid> = row.try_get("allowed_groups")?;
    Ok(ApiKeyRecord {
        api_key_id: ApiKeyId::from(id),
        project_id: ProjectId::from(project_id),
        org_id: OrgId::from(org_id),
        name: row.try_get("name")?,
        allowed_ips,
        allowed_models,
        allowed_groups,
        expires_at: row.try_get("expires_at")?,
        revoked_at: row.try_get("revoked_at")?,
    })
}

const API_KEY_COLUMNS: &str = "k.id, k.project_id, p.org_id, k.name, k.allowed_ips, \
    k.allowed_models, k.allowed_groups, k.expires_at, k.revoked_at";

#[async_trait]
impl ApiKeyRepo for PgApiKeyRepo {
    async fn find_by_hash(&self, hash: &str) -> DbResult<ApiKeyRecord> {
        let row = sqlx::query(&format!(
            "SELECT {API_KEY_COLUMNS} FROM api_keys k \
             JOIN projects p ON p.id = k.project_id \
             WHERE k.key_hash = $1"
        ))
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_record(&row)
    }

    async fn create(
        &self,
        project_id: ProjectId,
        name: &str,
        key_hash: &str,
        key_prefix: &str,
        key_last4: &str,
        created_by: UserId,
        allowed_models: &[String],
    ) -> DbResult<ApiKeyId> {
        let row = sqlx::query(
            "INSERT INTO api_keys \
                (project_id, name, key_hash, key_prefix, key_last4, created_by, allowed_models) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING id",
        )
        .bind(project_id.as_uuid())
        .bind(name)
        .bind(key_hash)
        .bind(key_prefix)
        .bind(key_last4)
        .bind(created_by.as_uuid())
        .bind(allowed_models)
        .fetch_one(&self.pool)
        .await?;
        let id: Uuid = row.try_get("id")?;
        Ok(ApiKeyId::from(id))
    }

    async fn list_in_project(&self, project_id: ProjectId) -> DbResult<Vec<ApiKeyRecord>> {
        let rows = sqlx::query(&format!(
            "SELECT {API_KEY_COLUMNS} FROM api_keys k \
             JOIN projects p ON p.id = k.project_id \
             WHERE k.project_id = $1 AND k.revoked_at IS NULL \
             ORDER BY k.created_at DESC"
        ))
        .bind(project_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_record).collect()
    }

    async fn list_summaries(&self, project_id: ProjectId) -> DbResult<Vec<ApiKeySummaryRecord>> {
        let rows = sqlx::query(
            "SELECT k.id, k.name, k.key_prefix, k.key_last4, k.allowed_models, \
                    k.created_at, k.last_used_at, k.revoked_at \
             FROM api_keys k \
             WHERE k.project_id = $1 \
             ORDER BY k.created_at DESC",
        )
        .bind(project_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                let id: Uuid = r.try_get("id")?;
                Ok(ApiKeySummaryRecord {
                    api_key_id: ApiKeyId::from(id),
                    name: r.try_get("name")?,
                    prefix: r.try_get("key_prefix")?,
                    last4: r.try_get("key_last4")?,
                    allowed_models: r.try_get("allowed_models")?,
                    created_at: r.try_get("created_at")?,
                    last_used_at: r.try_get("last_used_at")?,
                    revoked_at: r.try_get("revoked_at")?,
                })
            })
            .collect()
    }

    async fn revoke(&self, id: ApiKeyId, by: UserId, reason: Option<&str>) -> DbResult<()> {
        sqlx::query(
            "UPDATE api_keys SET revoked_at = NOW(), revoked_by = $2, revoked_reason = $3 \
             WHERE id = $1 AND revoked_at IS NULL",
        )
        .bind(id.as_uuid())
        .bind(by.as_uuid())
        .bind(reason)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn touch_used(
        &self,
        id: ApiKeyId,
        at: DateTime<Utc>,
        ip: Option<std::net::IpAddr>,
    ) -> DbResult<()> {
        let ip_net = ip.map(IpNetwork::from);
        sqlx::query(
            "UPDATE api_keys \
             SET last_used_at = $2, last_used_ip = $3, use_count = use_count + 1 \
             WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(at)
        .bind(ip_net)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
