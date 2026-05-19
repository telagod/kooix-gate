//! UserSessionRepo — 控制台 refresh token 会话持久化。
//!
//! 关键不变式：
//! - 只存 refresh token 的 SHA-256 hash，不落明文。
//! - refresh 轮转必须带 old hash 条件更新，旧 refresh token 立即失效。
//! - revoked / expired session 不参与列表和 refresh 校验。

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gate_core::id::UserId;
use parking_lot::RwLock;
use sqlx::types::ipnetwork::IpNetwork;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::net::IpAddr;
use subtle::ConstantTimeEq;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct UserSessionRecord {
    pub id: Uuid,
    pub user_id: UserId,
    pub refresh_token_hash: String,
    pub user_agent: Option<String>,
    pub ip: Option<IpAddr>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl UserSessionRecord {
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && self.expires_at > now
    }

    pub fn refresh_hash_matches(&self, candidate: &str) -> bool {
        constant_time_eq(&self.refresh_token_hash, candidate)
    }
}

#[derive(Debug, Clone)]
pub struct UserSessionCreate {
    pub id: Uuid,
    pub user_id: UserId,
    pub refresh_token_hash: String,
    pub user_agent: Option<String>,
    pub ip: Option<IpAddr>,
    pub expires_at: DateTime<Utc>,
}

#[async_trait]
pub trait UserSessionRepo: Send + Sync + 'static {
    async fn create(&self, rec: UserSessionCreate) -> DbResult<UserSessionRecord>;
    async fn find_active(&self, session_id: Uuid) -> DbResult<UserSessionRecord>;
    async fn rotate_refresh_hash(
        &self,
        session_id: Uuid,
        old_hash: &str,
        new_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> DbResult<UserSessionRecord>;
    async fn revoke(&self, session_id: Uuid) -> DbResult<()>;
    async fn revoke_for_user(&self, user_id: UserId, session_id: Uuid) -> DbResult<()>;
    async fn revoke_user_sessions(&self, user_id: UserId) -> DbResult<u64>;
    async fn list_active_for_user(&self, user_id: UserId) -> DbResult<Vec<UserSessionRecord>>;
}

pub struct PgUserSessionRepo {
    pool: PgPool,
}

impl PgUserSessionRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const SESSION_COLUMNS: &str = "id, user_id, refresh_token_hash, user_agent, ip, \
    created_at, last_used_at, expires_at, revoked_at";

fn constant_time_eq(left: &str, right: &str) -> bool {
    let a = left.as_bytes();
    let b = right.as_bytes();
    a.len() == b.len() && a.ct_eq(b).into()
}

fn row_to_session(row: &sqlx::postgres::PgRow) -> DbResult<UserSessionRecord> {
    let user_id: Uuid = row.try_get("user_id")?;
    let ip = row
        .try_get::<Option<IpNetwork>, _>("ip")?
        .map(|net| net.ip());
    Ok(UserSessionRecord {
        id: row.try_get("id")?,
        user_id: UserId::from(user_id),
        refresh_token_hash: row.try_get("refresh_token_hash")?,
        user_agent: row.try_get("user_agent")?,
        ip,
        created_at: row.try_get("created_at")?,
        last_used_at: row.try_get("last_used_at")?,
        expires_at: row.try_get("expires_at")?,
        revoked_at: row.try_get("revoked_at")?,
    })
}

#[async_trait]
impl UserSessionRepo for PgUserSessionRepo {
    async fn create(&self, rec: UserSessionCreate) -> DbResult<UserSessionRecord> {
        let ip = rec.ip.map(IpNetwork::from);
        let row = sqlx::query(&format!(
            "INSERT INTO user_sessions \
                (id, user_id, refresh_token_hash, user_agent, ip, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING {SESSION_COLUMNS}"
        ))
        .bind(rec.id)
        .bind(rec.user_id.as_uuid())
        .bind(&rec.refresh_token_hash)
        .bind(&rec.user_agent)
        .bind(ip)
        .bind(rec.expires_at)
        .fetch_one(&self.pool)
        .await?;
        row_to_session(&row)
    }

    async fn find_active(&self, session_id: Uuid) -> DbResult<UserSessionRecord> {
        let row = sqlx::query(&format!(
            "SELECT {SESSION_COLUMNS} FROM user_sessions \
             WHERE id = $1 AND revoked_at IS NULL AND expires_at > NOW()"
        ))
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_session(&row)
    }

    async fn rotate_refresh_hash(
        &self,
        session_id: Uuid,
        old_hash: &str,
        new_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> DbResult<UserSessionRecord> {
        let row = sqlx::query(&format!(
            "UPDATE user_sessions \
             SET refresh_token_hash = $2, last_used_at = NOW(), expires_at = $3 \
             WHERE id = $1 \
               AND refresh_token_hash = $4 \
               AND revoked_at IS NULL \
               AND expires_at > NOW() \
             RETURNING {SESSION_COLUMNS}"
        ))
        .bind(session_id)
        .bind(new_hash)
        .bind(expires_at)
        .bind(old_hash)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_session(&row)
    }

    async fn revoke(&self, session_id: Uuid) -> DbResult<()> {
        let result = sqlx::query(
            "UPDATE user_sessions SET revoked_at = COALESCE(revoked_at, NOW()) WHERE id = $1",
        )
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    async fn revoke_for_user(&self, user_id: UserId, session_id: Uuid) -> DbResult<()> {
        let result = sqlx::query(
            "UPDATE user_sessions SET revoked_at = COALESCE(revoked_at, NOW()) \
             WHERE id = $1 AND user_id = $2",
        )
        .bind(session_id)
        .bind(user_id.as_uuid())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    async fn revoke_user_sessions(&self, user_id: UserId) -> DbResult<u64> {
        let result = sqlx::query(
            "UPDATE user_sessions SET revoked_at = COALESCE(revoked_at, NOW()) \
             WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > NOW()",
        )
        .bind(user_id.as_uuid())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    async fn list_active_for_user(&self, user_id: UserId) -> DbResult<Vec<UserSessionRecord>> {
        let rows = sqlx::query(&format!(
            "SELECT {SESSION_COLUMNS} FROM user_sessions \
             WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > NOW() \
             ORDER BY last_used_at DESC, created_at DESC"
        ))
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_session).collect()
    }
}

#[derive(Default)]
pub struct InMemoryUserSessionRepo {
    inner: RwLock<HashMap<Uuid, UserSessionRecord>>,
}

impl InMemoryUserSessionRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl UserSessionRepo for InMemoryUserSessionRepo {
    async fn create(&self, rec: UserSessionCreate) -> DbResult<UserSessionRecord> {
        let mut g = self.inner.write();
        if g.values()
            .any(|s| s.refresh_token_hash == rec.refresh_token_hash)
        {
            return Err(DbError::Conflict(
                "refresh token hash already exists".into(),
            ));
        }
        let now = Utc::now();
        let session = UserSessionRecord {
            id: rec.id,
            user_id: rec.user_id,
            refresh_token_hash: rec.refresh_token_hash,
            user_agent: rec.user_agent,
            ip: rec.ip,
            created_at: now,
            last_used_at: now,
            expires_at: rec.expires_at,
            revoked_at: None,
        };
        g.insert(session.id, session.clone());
        Ok(session)
    }

    async fn find_active(&self, session_id: Uuid) -> DbResult<UserSessionRecord> {
        let now = Utc::now();
        let session = self
            .inner
            .read()
            .get(&session_id)
            .cloned()
            .ok_or(DbError::NotFound)?;
        if !session.is_active(now) {
            return Err(DbError::NotFound);
        }
        Ok(session)
    }

    async fn rotate_refresh_hash(
        &self,
        session_id: Uuid,
        old_hash: &str,
        new_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> DbResult<UserSessionRecord> {
        let mut g = self.inner.write();
        if g.values()
            .any(|s| s.id != session_id && s.refresh_token_hash == new_hash)
        {
            return Err(DbError::Conflict(
                "refresh token hash already exists".into(),
            ));
        }
        let session = g.get_mut(&session_id).ok_or(DbError::NotFound)?;
        if !session.is_active(Utc::now()) || !session.refresh_hash_matches(old_hash) {
            return Err(DbError::NotFound);
        }
        session.refresh_token_hash = new_hash.to_string();
        session.last_used_at = Utc::now();
        session.expires_at = expires_at;
        Ok(session.clone())
    }

    async fn revoke(&self, session_id: Uuid) -> DbResult<()> {
        let mut g = self.inner.write();
        let session = g.get_mut(&session_id).ok_or(DbError::NotFound)?;
        if session.revoked_at.is_none() {
            session.revoked_at = Some(Utc::now());
        }
        Ok(())
    }

    async fn revoke_for_user(&self, user_id: UserId, session_id: Uuid) -> DbResult<()> {
        let mut g = self.inner.write();
        let session = g.get_mut(&session_id).ok_or(DbError::NotFound)?;
        if session.user_id != user_id {
            return Err(DbError::NotFound);
        }
        if session.revoked_at.is_none() {
            session.revoked_at = Some(Utc::now());
        }
        Ok(())
    }

    async fn revoke_user_sessions(&self, user_id: UserId) -> DbResult<u64> {
        let mut affected = 0;
        let now = Utc::now();
        for session in self.inner.write().values_mut() {
            if session.user_id == user_id && session.is_active(now) {
                session.revoked_at = Some(now);
                affected += 1;
            }
        }
        Ok(affected)
    }

    async fn list_active_for_user(&self, user_id: UserId) -> DbResult<Vec<UserSessionRecord>> {
        let now = Utc::now();
        let mut sessions: Vec<_> = self
            .inner
            .read()
            .values()
            .filter(|s| s.user_id == user_id && s.is_active(now))
            .cloned()
            .collect();
        sessions.sort_by(|a, b| {
            b.last_used_at
                .cmp(&a.last_used_at)
                .then_with(|| b.created_at.cmp(&a.created_at))
        });
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[tokio::test]
    async fn in_memory_rotate_rejects_old_hash_and_revoke_all() {
        let repo = InMemoryUserSessionRepo::new();
        let user = UserId::new();
        let sid = Uuid::now_v7();
        repo.create(UserSessionCreate {
            id: sid,
            user_id: user,
            refresh_token_hash: "hash-a".into(),
            user_agent: Some("ua".into()),
            ip: Some("127.0.0.1".parse().unwrap()),
            expires_at: Utc::now() + Duration::hours(1),
        })
        .await
        .unwrap();

        assert!(
            repo.find_active(sid)
                .await
                .unwrap()
                .refresh_hash_matches("hash-a")
        );
        repo.rotate_refresh_hash(sid, "hash-a", "hash-b", Utc::now() + Duration::hours(1))
            .await
            .unwrap();
        assert!(
            repo.rotate_refresh_hash(sid, "hash-a", "hash-c", Utc::now())
                .await
                .is_err()
        );
        assert!(
            repo.find_active(sid)
                .await
                .unwrap()
                .refresh_hash_matches("hash-b")
        );

        let revoked = repo.revoke_user_sessions(user).await.unwrap();
        assert_eq!(revoked, 1);
        assert!(repo.find_active(sid).await.is_err());
    }
}
