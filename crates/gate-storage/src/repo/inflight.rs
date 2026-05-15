//! inflight_requests Repo — 记录飞行中请求的预扣状态，用于崩溃恢复。

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

pub struct InFlightRecord {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub api_key_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub model: String,
    pub estimated_cost_usd: f64,
    pub estimated_tokens: i32,
    pub started_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub quota_keys: Vec<String>,
    pub estimated_micros: Vec<i64>,
}

pub struct ExpiredInFlight {
    pub request_id: Uuid,
    pub quota_keys: Vec<String>,
    pub estimated_micros: Vec<i64>,
}

#[async_trait]
pub trait InFlightRepo: Send + Sync + 'static {
    async fn insert(&self, record: &InFlightRecord) -> crate::DbResult<()>;
    async fn delete(&self, request_id: Uuid) -> crate::DbResult<()>;
    async fn sweep_expired(&self) -> crate::DbResult<Vec<ExpiredInFlight>>;
}

pub struct PgInFlightRepo {
    pool: PgPool,
}

impl PgInFlightRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl InFlightRepo for PgInFlightRepo {
    async fn insert(&self, r: &InFlightRecord) -> crate::DbResult<()> {
        sqlx::query(
            "INSERT INTO inflight_requests \
             (request_id, project_id, api_key_id, channel_id, model, \
              estimated_cost_usd, estimated_tokens, started_at, expires_at, \
              quota_keys, estimated_micros) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             ON CONFLICT (request_id) DO NOTHING",
        )
        .bind(r.request_id)
        .bind(r.project_id)
        .bind(r.api_key_id)
        .bind(r.channel_id)
        .bind(&r.model)
        .bind(r.estimated_cost_usd)
        .bind(r.estimated_tokens)
        .bind(r.started_at)
        .bind(r.expires_at)
        .bind(&r.quota_keys)
        .bind(&r.estimated_micros)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete(&self, request_id: Uuid) -> crate::DbResult<()> {
        sqlx::query("DELETE FROM inflight_requests WHERE request_id = $1")
            .bind(request_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn sweep_expired(&self) -> crate::DbResult<Vec<ExpiredInFlight>> {
        let rows: Vec<(Uuid, Vec<String>, Vec<i64>)> = sqlx::query_as(
            "DELETE FROM inflight_requests WHERE expires_at < NOW() \
             RETURNING request_id, quota_keys, estimated_micros",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(request_id, quota_keys, estimated_micros)| ExpiredInFlight {
                    request_id,
                    quota_keys,
                    estimated_micros,
                },
            )
            .collect())
    }
}

#[derive(Default)]
pub struct InMemoryInFlightRepo;

impl InMemoryInFlightRepo {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl InFlightRepo for InMemoryInFlightRepo {
    async fn insert(&self, _record: &InFlightRecord) -> crate::DbResult<()> {
        Ok(())
    }
    async fn delete(&self, _request_id: Uuid) -> crate::DbResult<()> {
        Ok(())
    }
    async fn sweep_expired(&self) -> crate::DbResult<Vec<ExpiredInFlight>> {
        Ok(vec![])
    }
}
