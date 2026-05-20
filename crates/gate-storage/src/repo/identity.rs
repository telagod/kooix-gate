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
use serde_json::Value;
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
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct IdentityProviderCreate {
    pub id: Uuid,
    pub org_id: Option<OrgId>,
    pub name: String,
    pub slug: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret_enc: Vec<u8>,
    pub scopes: Vec<String>,
    pub email_claim: String,
    pub name_claim: String,
    pub subject_claim: String,
    pub auto_create_users: bool,
    pub auto_join_org_role: Option<String>,
    pub email_domain_allowlist: Vec<String>,
    pub enabled: bool,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct IdentityProviderUpdate {
    pub org_id: Option<Option<OrgId>>,
    pub name: Option<String>,
    pub slug: Option<String>,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub client_secret_enc: Option<Vec<u8>>,
    pub scopes: Option<Vec<String>>,
    pub email_claim: Option<String>,
    pub name_claim: Option<String>,
    pub subject_claim: Option<String>,
    pub auto_create_users: Option<bool>,
    pub auto_join_org_role: Option<Option<String>>,
    pub email_domain_allowlist: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub metadata: Option<Value>,
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

    async fn list(&self, limit: i64, offset: i64) -> DbResult<Vec<IdentityProviderRecord>>;

    async fn create(&self, input: IdentityProviderCreate) -> DbResult<IdentityProviderRecord>;

    async fn update(
        &self,
        id: Uuid,
        patch: IdentityProviderUpdate,
    ) -> DbResult<IdentityProviderRecord>;

    async fn soft_delete(&self, id: Uuid) -> DbResult<()>;
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
    email_domain_allowlist, enabled, metadata";

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
        metadata: row.try_get("metadata")?,
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

    async fn list(&self, limit: i64, offset: i64) -> DbResult<Vec<IdentityProviderRecord>> {
        let rows = sqlx::query(&format!(
            "SELECT {IDP_COLUMNS} FROM identity_providers \
             WHERE deleted_at IS NULL ORDER BY created_at DESC, id DESC LIMIT $1 OFFSET $2"
        ))
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_idp).collect()
    }

    async fn create(&self, input: IdentityProviderCreate) -> DbResult<IdentityProviderRecord> {
        let org_id = input.org_id.map(|o| *o.as_uuid());
        let row = sqlx::query(&format!(
            "INSERT INTO identity_providers \
                (id, org_id, name, slug, issuer, client_id, client_secret_enc, scopes, \
                 email_claim, name_claim, subject_claim, auto_create_users, auto_join_org_role, \
                 email_domain_allowlist, enabled, metadata) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16) \
             RETURNING {IDP_COLUMNS}"
        ))
        .bind(input.id)
        .bind(org_id)
        .bind(&input.name)
        .bind(&input.slug)
        .bind(&input.issuer)
        .bind(&input.client_id)
        .bind(&input.client_secret_enc)
        .bind(&input.scopes)
        .bind(&input.email_claim)
        .bind(&input.name_claim)
        .bind(&input.subject_claim)
        .bind(input.auto_create_users)
        .bind(&input.auto_join_org_role)
        .bind(&input.email_domain_allowlist)
        .bind(input.enabled)
        .bind(&input.metadata)
        .fetch_one(&self.pool)
        .await?;
        row_to_idp(&row)
    }

    async fn update(
        &self,
        id: Uuid,
        patch: IdentityProviderUpdate,
    ) -> DbResult<IdentityProviderRecord> {
        let current = self.find_by_id(id).await?;
        let org_id = patch.org_id.unwrap_or(current.org_id).map(|o| *o.as_uuid());
        let name = patch.name.unwrap_or(current.name);
        let slug = patch.slug.unwrap_or(current.slug);
        let issuer = patch.issuer.unwrap_or(current.issuer);
        let client_id = patch.client_id.unwrap_or(current.client_id);
        let client_secret_enc = patch.client_secret_enc.unwrap_or(current.client_secret_enc);
        let scopes = patch.scopes.unwrap_or(current.scopes);
        let email_claim = patch.email_claim.unwrap_or(current.email_claim);
        let name_claim = patch.name_claim.unwrap_or(current.name_claim);
        let subject_claim = patch.subject_claim.unwrap_or(current.subject_claim);
        let auto_create_users = patch.auto_create_users.unwrap_or(current.auto_create_users);
        let auto_join_org_role = patch
            .auto_join_org_role
            .unwrap_or(current.auto_join_org_role);
        let email_domain_allowlist = patch
            .email_domain_allowlist
            .unwrap_or(current.email_domain_allowlist);
        let enabled = patch.enabled.unwrap_or(current.enabled);
        let metadata = patch.metadata.unwrap_or(current.metadata);

        let row = sqlx::query(&format!(
            "UPDATE identity_providers SET \
                org_id = $2, name = $3, slug = $4, issuer = $5, client_id = $6, \
                client_secret_enc = $7, scopes = $8, email_claim = $9, name_claim = $10, \
                subject_claim = $11, auto_create_users = $12, auto_join_org_role = $13, \
                email_domain_allowlist = $14, enabled = $15, metadata = $16 \
             WHERE id = $1 AND deleted_at IS NULL RETURNING {IDP_COLUMNS}"
        ))
        .bind(id)
        .bind(org_id)
        .bind(&name)
        .bind(&slug)
        .bind(&issuer)
        .bind(&client_id)
        .bind(&client_secret_enc)
        .bind(&scopes)
        .bind(&email_claim)
        .bind(&name_claim)
        .bind(&subject_claim)
        .bind(auto_create_users)
        .bind(&auto_join_org_role)
        .bind(&email_domain_allowlist)
        .bind(enabled)
        .bind(&metadata)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_idp(&row)
    }

    async fn soft_delete(&self, id: Uuid) -> DbResult<()> {
        let res = sqlx::query(
            "UPDATE identity_providers SET deleted_at = NOW(), enabled = FALSE \
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
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

use parking_lot::RwLock;
use std::collections::HashMap;

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
        let mut g = self.inner.write();
        g.by_slug
            .insert((record.org_id, record.slug.to_lowercase()), record.id);
        g.by_id.insert(record.id, record);
    }
}

#[async_trait]
impl IdentityProviderRepo for InMemoryIdentityProviderRepo {
    async fn find_platform_by_slug(&self, slug: &str) -> DbResult<IdentityProviderRecord> {
        let g = self.inner.read();
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
        let g = self.inner.read();
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
            .by_id
            .get(&id)
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn list(&self, limit: i64, offset: i64) -> DbResult<Vec<IdentityProviderRecord>> {
        let mut rows: Vec<_> = self.inner.read().by_id.values().cloned().collect();
        rows.sort_by(|a, b| a.slug.cmp(&b.slug).then_with(|| a.id.cmp(&b.id)));
        Ok(rows
            .into_iter()
            .skip(offset.max(0) as usize)
            .take(limit.max(0) as usize)
            .collect())
    }

    async fn create(&self, input: IdentityProviderCreate) -> DbResult<IdentityProviderRecord> {
        let rec = IdentityProviderRecord {
            id: input.id,
            org_id: input.org_id,
            name: input.name,
            slug: input.slug,
            issuer: input.issuer,
            client_id: input.client_id,
            client_secret_enc: input.client_secret_enc,
            scopes: input.scopes,
            email_claim: input.email_claim,
            name_claim: input.name_claim,
            subject_claim: input.subject_claim,
            auto_create_users: input.auto_create_users,
            auto_join_org_role: input.auto_join_org_role,
            email_domain_allowlist: input.email_domain_allowlist,
            enabled: input.enabled,
            metadata: input.metadata,
        };
        let key = (rec.org_id, rec.slug.to_lowercase());
        let mut g = self.inner.write();
        if g.by_id.contains_key(&rec.id) || g.by_slug.contains_key(&key) {
            return Err(DbError::Conflict("identity provider already exists".into()));
        }
        g.by_slug.insert(key, rec.id);
        g.by_id.insert(rec.id, rec.clone());
        Ok(rec)
    }

    async fn update(
        &self,
        id: Uuid,
        patch: IdentityProviderUpdate,
    ) -> DbResult<IdentityProviderRecord> {
        let mut g = self.inner.write();
        let mut rec = g.by_id.get(&id).cloned().ok_or(DbError::NotFound)?;
        let old_key = (rec.org_id, rec.slug.to_lowercase());
        if let Some(org_id) = patch.org_id {
            rec.org_id = org_id;
        }
        if let Some(name) = patch.name {
            rec.name = name;
        }
        if let Some(slug) = patch.slug {
            rec.slug = slug;
        }
        if let Some(issuer) = patch.issuer {
            rec.issuer = issuer;
        }
        if let Some(client_id) = patch.client_id {
            rec.client_id = client_id;
        }
        if let Some(client_secret_enc) = patch.client_secret_enc {
            rec.client_secret_enc = client_secret_enc;
        }
        if let Some(scopes) = patch.scopes {
            rec.scopes = scopes;
        }
        if let Some(email_claim) = patch.email_claim {
            rec.email_claim = email_claim;
        }
        if let Some(name_claim) = patch.name_claim {
            rec.name_claim = name_claim;
        }
        if let Some(subject_claim) = patch.subject_claim {
            rec.subject_claim = subject_claim;
        }
        if let Some(auto_create_users) = patch.auto_create_users {
            rec.auto_create_users = auto_create_users;
        }
        if let Some(auto_join_org_role) = patch.auto_join_org_role {
            rec.auto_join_org_role = auto_join_org_role;
        }
        if let Some(email_domain_allowlist) = patch.email_domain_allowlist {
            rec.email_domain_allowlist = email_domain_allowlist;
        }
        if let Some(enabled) = patch.enabled {
            rec.enabled = enabled;
        }
        if let Some(metadata) = patch.metadata {
            rec.metadata = metadata;
        }
        let new_key = (rec.org_id, rec.slug.to_lowercase());
        if old_key != new_key {
            if g.by_slug.contains_key(&new_key) {
                return Err(DbError::Conflict(
                    "identity provider slug already exists".into(),
                ));
            }
            g.by_slug.remove(&old_key);
            g.by_slug.insert(new_key, id);
        }
        g.by_id.insert(id, rec.clone());
        Ok(rec)
    }

    async fn soft_delete(&self, id: Uuid) -> DbResult<()> {
        let mut g = self.inner.write();
        let rec = g.by_id.remove(&id).ok_or(DbError::NotFound)?;
        g.by_slug.remove(&(rec.org_id, rec.slug.to_lowercase()));
        Ok(())
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
        let mut g = self.inner.write();
        if g.contains_key(state_hash) {
            return Err(DbError::Conflict("oidc state already exists".into()));
        }
        g.insert(state_hash.to_string(), rec);
        Ok(())
    }

    async fn consume(&self, state_hash: &str) -> DbResult<OidcStateRecord> {
        self.inner
            .write()
            .remove(state_hash)
            .ok_or(DbError::NotFound)
    }

    async fn cleanup_expired(&self) -> DbResult<u64> {
        let now = Utc::now();
        let mut g = self.inner.write();
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
            .insert((provider_id, subject.to_string()), rec);
        Ok(())
    }

    async fn touch_last_login(&self, provider_id: Uuid, subject: &str) -> DbResult<()> {
        let mut g = self.inner.write();
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
            metadata: serde_json::json!({}),
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
    async fn memory_idp_create_update_list_and_soft_delete() {
        let repo = InMemoryIdentityProviderRepo::new();
        let id = Uuid::now_v7();

        let created = repo
            .create(IdentityProviderCreate {
                id,
                org_id: None,
                name: "Google".into(),
                slug: "google".into(),
                issuer: "https://accounts.example.com".into(),
                client_id: "cid".into(),
                client_secret_enc: vec![1, 2, 3],
                scopes: vec!["openid".into(), "email".into()],
                email_claim: "email".into(),
                name_claim: "name".into(),
                subject_claim: "sub".into(),
                auto_create_users: true,
                auto_join_org_role: None,
                email_domain_allowlist: vec!["example.com".into()],
                enabled: true,
                metadata: serde_json::json!({"redirect_policy":{"allow_relative":true}}),
            })
            .await
            .unwrap();
        assert_eq!(created.id, id);
        assert_eq!(repo.list(10, 0).await.unwrap().len(), 1);

        let updated = repo
            .update(
                id,
                IdentityProviderUpdate {
                    org_id: None,
                    name: Some("Google Workspace".into()),
                    slug: Some("workspace".into()),
                    issuer: None,
                    client_id: None,
                    client_secret_enc: None,
                    scopes: None,
                    email_claim: None,
                    name_claim: None,
                    subject_claim: None,
                    auto_create_users: Some(false),
                    auto_join_org_role: Some(Some("member".into())),
                    email_domain_allowlist: None,
                    enabled: Some(false),
                    metadata: Some(serde_json::json!({"redirect_policy":{"allowed_origins":["https://console.example.com"]}})),
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.slug, "workspace");
        assert!(!updated.enabled);
        assert!(repo.find_platform_by_slug("google").await.is_err());

        repo.soft_delete(id).await.unwrap();
        assert!(repo.find_by_id(id).await.is_err());
        assert!(repo.list(10, 0).await.unwrap().is_empty());
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
