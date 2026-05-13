//! Identity Provider / User Identity / OIDC login state repositories.
//!
//! 设计要点：
//! - `IdentityProviderRecord` 携带 `client_secret_enc` 原文（envelope ciphertext），
//!   解密留给 server 层（避免 gate-storage 依赖 gate-crypto）。
//! - `OidcStateRepo::consume` 用 `DELETE ... RETURNING` 保证一次性消费——
//!   即使并发回调也只有一次成功。
//! - `UserIdentityRepo::link` 走 UPSERT，绑定后续重复登录无副作用。
//!
//! 一致性兜底由表上的 unique 约束 + RLS 保证；这一层只负责 SQL。

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use gate_core::id::{OrgId, UserId};
use sqlx::{PgPool, Row};
use uuid::Uuid;

// ----------------------------------------------------------------------------
// 数据结构
// ----------------------------------------------------------------------------

/// 身份提供者完整快照（含加密 client_secret 原文）。
///
/// 字段对齐 `identity_providers` 表。`org_id` 为 `None` 即平台级（共享）IdP。
#[derive(Debug, Clone)]
pub struct IdentityProviderRecord {
    pub id: Uuid,
    pub org_id: Option<OrgId>,
    pub name: String,
    pub slug: String,
    pub issuer: String,
    pub client_id: String,
    /// envelope-encrypted client_secret —— 由调用方解密
    pub client_secret_enc: Vec<u8>,
    pub scopes: Vec<String>,
    pub email_claim: String,
    pub name_claim: String,
    pub subject_claim: String,
    pub auto_create_users: bool,
    pub auto_join_org_role: Option<String>,
    pub email_domain_allowlist: Vec<String>,
    pub enabled: bool,
}

/// OIDC login state 一次性快照。`consume` 返回后此行已被删除。
#[derive(Debug, Clone)]
pub struct OidcStateRecord {
    pub provider_id: Uuid,
    pub pkce_verifier: String,
    pub nonce: String,
    pub redirect_to: Option<String>,
    pub expires_at: DateTime<Utc>,
}

impl OidcStateRecord {
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at <= now
    }
}

/// 用户外部身份绑定快照（user_identities 行）。
#[derive(Debug, Clone)]
pub struct UserIdentityRecord {
    pub user_id: UserId,
    pub provider_id: Uuid,
    pub subject: String,
    pub email_at_link: Option<String>,
    pub last_login_at: Option<DateTime<Utc>>,
}

// ----------------------------------------------------------------------------
// Traits
// ----------------------------------------------------------------------------

#[async_trait]
pub trait IdentityProviderRepo: Send + Sync + 'static {
    /// 平台级 IdP（org_id IS NULL）按 slug 查。
    async fn find_platform_by_slug(&self, slug: &str) -> DbResult<IdentityProviderRecord>;

    /// Org 级 IdP 按 (org_id, slug) 查。
    async fn find_by_org_slug(&self, org_id: OrgId, slug: &str)
    -> DbResult<IdentityProviderRecord>;

    async fn find_by_id(&self, id: Uuid) -> DbResult<IdentityProviderRecord>;
}

#[async_trait]
pub trait OidcStateRepo: Send + Sync + 'static {
    /// 写入一条 login state。`state_hash` 必须唯一（PK）。
    async fn save(
        &self,
        state_hash: &str,
        provider_id: Uuid,
        pkce_verifier: &str,
        nonce: &str,
        redirect_to: Option<&str>,
        ttl: Duration,
    ) -> DbResult<()>;

    /// 一次性消费：DELETE RETURNING。读到即删，重放无效。
    async fn consume(&self, state_hash: &str) -> DbResult<OidcStateRecord>;

    /// 后台清扫：删除已过期项。返回删除条数。
    async fn cleanup_expired(&self) -> DbResult<u64>;
}

#[async_trait]
pub trait UserIdentityRepo: Send + Sync + 'static {
    /// 用 (provider_id, subject) 反查已绑定的 user。
    async fn find_by_provider_subject(
        &self,
        provider_id: Uuid,
        subject: &str,
    ) -> DbResult<Option<UserIdentityRecord>>;

    /// 绑定/更新外部身份。`(provider_id, subject)` 冲突时 UPSERT。
    async fn link(
        &self,
        user_id: UserId,
        provider_id: Uuid,
        subject: &str,
        email: Option<&str>,
        raw_claims: serde_json::Value,
    ) -> DbResult<()>;

    /// 仅更新 last_login_at；不存在则视为已删除，返回 NotFound。
    async fn touch_last_login(&self, provider_id: Uuid, subject: &str) -> DbResult<()>;
}

// ----------------------------------------------------------------------------
// Pg 实现
// ----------------------------------------------------------------------------

const IDP_COLUMNS: &str = "id, org_id, name, slug, issuer, client_id, client_secret_enc, \
    scopes, email_claim, name_claim, subject_claim, auto_create_users, auto_join_org_role, \
    email_domain_allowlist, enabled";

fn row_to_idp(row: &sqlx::postgres::PgRow) -> DbResult<IdentityProviderRecord> {
    let id: Uuid = row.try_get("id")?;
    let org_id: Option<Uuid> = row.try_get("org_id")?;
    Ok(IdentityProviderRecord {
        id,
        org_id: org_id.map(OrgId::from),
        name: row.try_get("name")?,
        slug: row.try_get("slug")?,
        issuer: row.try_get("issuer")?,
        client_id: row.try_get("client_id")?,
        client_secret_enc: row.try_get("client_secret_enc")?,
        scopes: row.try_get("scopes")?,
        email_claim: row.try_get("email_claim")?,
        name_claim: row.try_get("name_claim")?,
        subject_claim: row.try_get("subject_claim")?,
        auto_create_users: row.try_get("auto_create_users")?,
        auto_join_org_role: row.try_get("auto_join_org_role")?,
        email_domain_allowlist: row.try_get("email_domain_allowlist")?,
        enabled: row.try_get("enabled")?,
    })
}

pub struct PgIdentityProviderRepo {
    pool: PgPool,
}

impl PgIdentityProviderRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl IdentityProviderRepo for PgIdentityProviderRepo {
    async fn find_platform_by_slug(&self, slug: &str) -> DbResult<IdentityProviderRecord> {
        let row = sqlx::query(&format!(
            "SELECT {IDP_COLUMNS} FROM identity_providers \
             WHERE slug = $1 AND org_id IS NULL AND deleted_at IS NULL AND enabled = TRUE"
        ))
        .bind(slug)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_idp(&row)
    }

    async fn find_by_org_slug(
        &self,
        org_id: OrgId,
        slug: &str,
    ) -> DbResult<IdentityProviderRecord> {
        let row = sqlx::query(&format!(
            "SELECT {IDP_COLUMNS} FROM identity_providers \
             WHERE slug = $1 AND org_id = $2 AND deleted_at IS NULL AND enabled = TRUE"
        ))
        .bind(slug)
        .bind(org_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_idp(&row)
    }

    async fn find_by_id(&self, id: Uuid) -> DbResult<IdentityProviderRecord> {
        let row = sqlx::query(&format!(
            "SELECT {IDP_COLUMNS} FROM identity_providers \
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_idp(&row)
    }
}

// ---- OidcStateRepo --------------------------------------------------------

pub struct PgOidcStateRepo {
    pool: PgPool,
}

impl PgOidcStateRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OidcStateRepo for PgOidcStateRepo {
    async fn save(
        &self,
        state_hash: &str,
        provider_id: Uuid,
        pkce_verifier: &str,
        nonce: &str,
        redirect_to: Option<&str>,
        ttl: Duration,
    ) -> DbResult<()> {
        let expires_at = Utc::now() + ttl;
        sqlx::query(
            "INSERT INTO oidc_login_states \
                (state_hash, provider_id, pkce_verifier, nonce, redirect_to, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(state_hash)
        .bind(provider_id)
        .bind(pkce_verifier)
        .bind(nonce)
        .bind(redirect_to)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn consume(&self, state_hash: &str) -> DbResult<OidcStateRecord> {
        let row = sqlx::query(
            "DELETE FROM oidc_login_states WHERE state_hash = $1 \
             RETURNING provider_id, pkce_verifier, nonce, redirect_to, expires_at",
        )
        .bind(state_hash)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;

        Ok(OidcStateRecord {
            provider_id: row.try_get("provider_id")?,
            pkce_verifier: row.try_get("pkce_verifier")?,
            nonce: row.try_get("nonce")?,
            redirect_to: row.try_get("redirect_to")?,
            expires_at: row.try_get("expires_at")?,
        })
    }

    async fn cleanup_expired(&self) -> DbResult<u64> {
        let res = sqlx::query("DELETE FROM oidc_login_states WHERE expires_at <= NOW()")
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }
}

// ---- UserIdentityRepo -----------------------------------------------------

pub struct PgUserIdentityRepo {
    pool: PgPool,
}

impl PgUserIdentityRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserIdentityRepo for PgUserIdentityRepo {
    async fn find_by_provider_subject(
        &self,
        provider_id: Uuid,
        subject: &str,
    ) -> DbResult<Option<UserIdentityRecord>> {
        let row = sqlx::query(
            "SELECT user_id, provider_id, subject, email_at_link, last_login_at \
             FROM user_identities WHERE provider_id = $1 AND subject = $2",
        )
        .bind(provider_id)
        .bind(subject)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            None => Ok(None),
            Some(r) => {
                let user_id: Uuid = r.try_get("user_id")?;
                Ok(Some(UserIdentityRecord {
                    user_id: UserId::from(user_id),
                    provider_id: r.try_get("provider_id")?,
                    subject: r.try_get("subject")?,
                    email_at_link: r.try_get("email_at_link")?,
                    last_login_at: r.try_get("last_login_at")?,
                }))
            }
        }
    }

    async fn link(
        &self,
        user_id: UserId,
        provider_id: Uuid,
        subject: &str,
        email: Option<&str>,
        raw_claims: serde_json::Value,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO user_identities \
                (user_id, provider_id, subject, email_at_link, raw_claims, last_login_at) \
             VALUES ($1, $2, $3, $4, $5, NOW()) \
             ON CONFLICT (provider_id, subject) DO UPDATE \
             SET email_at_link = EXCLUDED.email_at_link, \
                 raw_claims    = EXCLUDED.raw_claims, \
                 last_login_at = NOW()",
        )
        .bind(user_id.as_uuid())
        .bind(provider_id)
        .bind(subject)
        .bind(email)
        .bind(raw_claims)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn touch_last_login(&self, provider_id: Uuid, subject: &str) -> DbResult<()> {
        let res = sqlx::query(
            "UPDATE user_identities SET last_login_at = NOW() \
             WHERE provider_id = $1 AND subject = $2",
        )
        .bind(provider_id)
        .bind(subject)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// In-memory 实现（测试用）
// ----------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Default)]
pub struct InMemoryIdentityProviderRepo {
    inner: RwLock<IdpInner>,
}

#[derive(Default)]
struct IdpInner {
    by_id: HashMap<Uuid, IdentityProviderRecord>,
    /// (org_id?, slug.lower()) → idp_id
    by_slug: HashMap<(Option<OrgId>, String), Uuid>,
}

impl InMemoryIdentityProviderRepo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed(&self, record: IdentityProviderRecord) {
        let mut g = self.inner.write().unwrap();
        g.by_slug
            .insert((record.org_id, record.slug.to_lowercase()), record.id);
        g.by_id.insert(record.id, record);
    }
}

#[async_trait]
impl IdentityProviderRepo for InMemoryIdentityProviderRepo {
    async fn find_platform_by_slug(&self, slug: &str) -> DbResult<IdentityProviderRecord> {
        let g = self.inner.read().unwrap();
        let id = g
            .by_slug
            .get(&(None, slug.to_lowercase()))
            .ok_or(DbError::NotFound)?;
        g.by_id
            .get(id)
            .filter(|r| r.enabled)
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn find_by_org_slug(
        &self,
        org_id: OrgId,
        slug: &str,
    ) -> DbResult<IdentityProviderRecord> {
        let g = self.inner.read().unwrap();
        let id = g
            .by_slug
            .get(&(Some(org_id), slug.to_lowercase()))
            .ok_or(DbError::NotFound)?;
        g.by_id
            .get(id)
            .filter(|r| r.enabled)
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn find_by_id(&self, id: Uuid) -> DbResult<IdentityProviderRecord> {
        self.inner
            .read()
            .unwrap()
            .by_id
            .get(&id)
            .cloned()
            .ok_or(DbError::NotFound)
    }
}

#[derive(Default)]
pub struct InMemoryOidcStateRepo {
    inner: RwLock<HashMap<String, OidcStateRecord>>,
}

impl InMemoryOidcStateRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl OidcStateRepo for InMemoryOidcStateRepo {
    async fn save(
        &self,
        state_hash: &str,
        provider_id: Uuid,
        pkce_verifier: &str,
        nonce: &str,
        redirect_to: Option<&str>,
        ttl: Duration,
    ) -> DbResult<()> {
        let rec = OidcStateRecord {
            provider_id,
            pkce_verifier: pkce_verifier.to_string(),
            nonce: nonce.to_string(),
            redirect_to: redirect_to.map(String::from),
            expires_at: Utc::now() + ttl,
        };
        let mut g = self.inner.write().unwrap();
        if g.contains_key(state_hash) {
            return Err(DbError::Conflict("oidc state already exists".into()));
        }
        g.insert(state_hash.to_string(), rec);
        Ok(())
    }

    async fn consume(&self, state_hash: &str) -> DbResult<OidcStateRecord> {
        self.inner
            .write()
            .unwrap()
            .remove(state_hash)
            .ok_or(DbError::NotFound)
    }

    async fn cleanup_expired(&self) -> DbResult<u64> {
        let now = Utc::now();
        let mut g = self.inner.write().unwrap();
        let before = g.len();
        g.retain(|_, v| v.expires_at > now);
        Ok((before - g.len()) as u64)
    }
}

#[derive(Default)]
pub struct InMemoryUserIdentityRepo {
    inner: RwLock<HashMap<(Uuid, String), UserIdentityRecord>>,
}

impl InMemoryUserIdentityRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl UserIdentityRepo for InMemoryUserIdentityRepo {
    async fn find_by_provider_subject(
        &self,
        provider_id: Uuid,
        subject: &str,
    ) -> DbResult<Option<UserIdentityRecord>> {
        Ok(self
            .inner
            .read()
            .unwrap()
            .get(&(provider_id, subject.to_string()))
            .cloned())
    }

    async fn link(
        &self,
        user_id: UserId,
        provider_id: Uuid,
        subject: &str,
        email: Option<&str>,
        _raw_claims: serde_json::Value,
    ) -> DbResult<()> {
        let rec = UserIdentityRecord {
            user_id,
            provider_id,
            subject: subject.to_string(),
            email_at_link: email.map(String::from),
            last_login_at: Some(Utc::now()),
        };
        self.inner
            .write()
            .unwrap()
            .insert((provider_id, subject.to_string()), rec);
        Ok(())
    }

    async fn touch_last_login(&self, provider_id: Uuid, subject: &str) -> DbResult<()> {
        let mut g = self.inner.write().unwrap();
        let rec = g
            .get_mut(&(provider_id, subject.to_string()))
            .ok_or(DbError::NotFound)?;
        rec.last_login_at = Some(Utc::now());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_idp(org: Option<OrgId>, slug: &str) -> IdentityProviderRecord {
        IdentityProviderRecord {
            id: Uuid::now_v7(),
            org_id: org,
            name: format!("idp-{slug}"),
            slug: slug.to_string(),
            issuer: "https://example.com".into(),
            client_id: "cid".into(),
            client_secret_enc: vec![1, 2, 3],
            scopes: vec!["openid".into()],
            email_claim: "email".into(),
            name_claim: "name".into(),
            subject_claim: "sub".into(),
            auto_create_users: true,
            auto_join_org_role: None,
            email_domain_allowlist: vec![],
            enabled: true,
        }
    }

    #[tokio::test]
    async fn memory_idp_lookup_platform_vs_org() {
        let repo = InMemoryIdentityProviderRepo::new();
        let org = OrgId::new();
        let plat = sample_idp(None, "google");
        let org_idp = sample_idp(Some(org), "okta");
        repo.seed(plat.clone());
        repo.seed(org_idp.clone());

        let found = repo.find_platform_by_slug("google").await.unwrap();
        assert_eq!(found.id, plat.id);
        let found = repo.find_by_org_slug(org, "okta").await.unwrap();
        assert_eq!(found.id, org_idp.id);

        // 平台级查询不命中 org 级 IdP
        assert!(repo.find_platform_by_slug("okta").await.is_err());
    }

    #[tokio::test]
    async fn memory_oidc_state_consume_once() {
        let repo = InMemoryOidcStateRepo::new();
        let pid = Uuid::now_v7();
        repo.save("h1", pid, "v", "n", Some("/back"), Duration::seconds(60))
            .await
            .unwrap();
        let rec = repo.consume("h1").await.unwrap();
        assert_eq!(rec.provider_id, pid);
        // 二次消费应失败（防重放）
        assert!(repo.consume("h1").await.is_err());
    }

    #[tokio::test]
    async fn memory_user_identity_link_and_lookup() {
        let repo = InMemoryUserIdentityRepo::new();
        let uid = UserId::new();
        let pid = Uuid::now_v7();
        repo.link(uid, pid, "sub-42", Some("a@b.c"), serde_json::json!({}))
            .await
            .unwrap();
        let got = repo
            .find_by_provider_subject(pid, "sub-42")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.user_id, uid);
        repo.touch_last_login(pid, "sub-42").await.unwrap();
    }
}
