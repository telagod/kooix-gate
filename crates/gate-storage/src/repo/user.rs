//! UserRepo — 用户读写。
//!
//! 设计要点：
//! - 登录路径只返回必要字段，避免把 `password_hash` 等敏感列带到高层。
//! - 软删用 `deleted_at IS NULL` 过滤；高层看不到"墓碑"。

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gate_core::id::UserId;
use gate_core::identity::{User, UserStatus};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[async_trait]
pub trait UserRepo: Send + Sync + 'static {
    async fn find_by_id(&self, id: UserId) -> DbResult<User>;
    async fn find_by_email(&self, email: &str) -> DbResult<User>;

    /// 用于登录校验：返回 (user, password_hash)。`password_hash` 可能为 None（SSO 用户）。
    async fn find_credentials(&self, email: &str) -> DbResult<(User, Option<String>)>;

    /// 创建用户（明文密码在 gate-auth 里已 Argon2 哈希）。
    async fn create(
        &self,
        email: &str,
        password_hash: Option<&str>,
        display_name: Option<&str>,
    ) -> DbResult<User>;

    async fn mark_last_login(
        &self,
        id: UserId,
        at: DateTime<Utc>,
        ip: Option<std::net::IpAddr>,
    ) -> DbResult<()>;

    /// 登录失败 +1，返回当前失败次数。调用方自行判断是否 lock。
    async fn bump_failed_login(&self, id: UserId) -> DbResult<i32>;

    async fn reset_failed_login(&self, id: UserId) -> DbResult<()>;
}

pub struct PgUserRepo {
    pool: PgPool,
}

impl PgUserRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn parse_status(s: &str) -> DbResult<UserStatus> {
    match s {
        "active" => Ok(UserStatus::Active),
        "suspended" => Ok(UserStatus::Suspended),
        "deleted" => Ok(UserStatus::Deleted),
        "pending_verification" => Ok(UserStatus::PendingVerification),
        other => Err(DbError::Internal(format!("unknown user status: {other}"))),
    }
}

fn row_to_user(row: &sqlx::postgres::PgRow) -> DbResult<User> {
    let id: Uuid = row.try_get("id")?;
    let status: String = row.try_get("status")?;
    Ok(User {
        id: UserId::from(id),
        email: row.try_get("email")?,
        display_name: row.try_get("display_name")?,
        status: parse_status(&status)?,
        mfa_enabled: row.try_get("mfa_enabled")?,
        email_verified_at: row.try_get("email_verified_at")?,
        last_login_at: row.try_get("last_login_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

const USER_COLUMNS: &str = "id, email, display_name, status, mfa_enabled, \
    email_verified_at, last_login_at, created_at, updated_at";

#[async_trait]
impl UserRepo for PgUserRepo {
    async fn find_by_id(&self, id: UserId) -> DbResult<User> {
        let row = sqlx::query(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_user(&row)
    }

    async fn find_by_email(&self, email: &str) -> DbResult<User> {
        let row = sqlx::query(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE email = $1 AND deleted_at IS NULL"
        ))
        .bind(email)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_user(&row)
    }

    async fn find_credentials(&self, email: &str) -> DbResult<(User, Option<String>)> {
        let row = sqlx::query(&format!(
            "SELECT {USER_COLUMNS}, password_hash FROM users \
             WHERE email = $1 AND deleted_at IS NULL"
        ))
        .bind(email)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        let user = row_to_user(&row)?;
        let ph: Option<String> = row.try_get("password_hash")?;
        Ok((user, ph))
    }

    async fn create(
        &self,
        email: &str,
        password_hash: Option<&str>,
        display_name: Option<&str>,
    ) -> DbResult<User> {
        let row = sqlx::query(&format!(
            "INSERT INTO users (email, password_hash, display_name) \
             VALUES ($1, $2, $3) \
             RETURNING {USER_COLUMNS}"
        ))
        .bind(email)
        .bind(password_hash)
        .bind(display_name)
        .fetch_one(&self.pool)
        .await?;
        row_to_user(&row)
    }

    async fn mark_last_login(
        &self,
        id: UserId,
        at: DateTime<Utc>,
        ip: Option<std::net::IpAddr>,
    ) -> DbResult<()> {
        let ip_net = ip.map(sqlx::types::ipnetwork::IpNetwork::from);
        sqlx::query(
            "UPDATE users SET last_login_at = $2, last_login_ip = $3, failed_logins = 0 \
             WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(at)
        .bind(ip_net)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn bump_failed_login(&self, id: UserId) -> DbResult<i32> {
        let row = sqlx::query(
            "UPDATE users SET failed_logins = failed_logins + 1 \
             WHERE id = $1 RETURNING failed_logins",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        Ok(row.try_get("failed_logins")?)
    }

    async fn reset_failed_login(&self, id: UserId) -> DbResult<()> {
        sqlx::query("UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
