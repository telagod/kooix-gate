//! `PgLoader` — 用 5 个 Repo 把 `AuthContextLoader` 实现成真 DB 版本。
//!
//! 与 [`InMemoryLoader`] 的差异：
//! - 用户路径：MembershipRepo 一次拉满 + 平台角色
//! - API key 路径：按 hash 查记录，做 IP / 过期 / 撤销 三道闸
//! - touch_used 是后置写，失败只 warn，不影响认证结果（认证已成功）
//!
//! [`InMemoryLoader`]: super::loader::InMemoryLoader

use crate::loader::{AuthContextLoader, LoaderError};
use async_trait::async_trait;
use chrono::Utc;
use gate_auth::AuthContext;
use gate_core::id::{OrgId, UserId};
use gate_storage::{
    ApiKeyRepo, DbError, MembershipRepo, PgApiKeyRepo, PgMembershipRepo, PgUserRepo, UserRepo,
};
use std::net::IpAddr;
use std::sync::Arc;

pub struct PgLoader {
    users: Arc<dyn UserRepo>,
    memberships: Arc<dyn MembershipRepo>,
    api_keys: Arc<dyn ApiKeyRepo>,
}

impl PgLoader {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self {
            users: Arc::new(PgUserRepo::new(pool.clone())),
            memberships: Arc::new(PgMembershipRepo::new(pool.clone())),
            api_keys: Arc::new(PgApiKeyRepo::new(pool)),
        }
    }

    /// 注入自定义 Repo（测试时换桩用）。
    pub fn from_parts(
        users: Arc<dyn UserRepo>,
        memberships: Arc<dyn MembershipRepo>,
        api_keys: Arc<dyn ApiKeyRepo>,
    ) -> Self {
        Self {
            users,
            memberships,
            api_keys,
        }
    }
}

impl From<DbError> for LoaderError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::NotFound => Self::Internal("db row not found".into()),
            other => Self::Internal(other.to_string()),
        }
    }
}

#[async_trait]
impl AuthContextLoader for PgLoader {
    async fn load_user(
        &self,
        user_id: UserId,
        session_id: uuid::Uuid,
        current_org: Option<OrgId>,
    ) -> Result<AuthContext, LoaderError> {
        // 1. 拉用户行 — 不存在或软删 → UserUnavailable
        let user = self.users.find_by_id(user_id).await.map_err(|e| match e {
            DbError::NotFound => LoaderError::UserUnavailable,
            other => LoaderError::Internal(other.to_string()),
        })?;

        // suspended / pending / deleted 都不应能登录
        if !matches!(user.status, gate_core::identity::UserStatus::Active) {
            return Err(LoaderError::UserUnavailable);
        }

        // 2. 一次拉全部成员关系
        let m = self.memberships.load_for_user(user_id).await?;

        Ok(AuthContext::user(
            user_id,
            session_id,
            m.orgs,
            m.projects,
            m.platform,
            current_org,
        ))
    }

    async fn load_api_key(
        &self,
        plaintext: &str,
        client_ip: Option<IpAddr>,
    ) -> Result<AuthContext, LoaderError> {
        let hash = gate_auth::api_key::hash(plaintext);

        let rec = self
            .api_keys
            .find_by_hash(&hash)
            .await
            .map_err(|e| match e {
                DbError::NotFound => LoaderError::ApiKeyInvalid,
                other => LoaderError::Internal(other.to_string()),
            })?;

        // 撤销
        if rec.is_revoked() {
            return Err(LoaderError::ApiKeyRevoked);
        }

        let now = Utc::now();

        // 过期视同撤销（业务上不区分）
        if rec.is_expired(now) {
            return Err(LoaderError::ApiKeyRevoked);
        }

        // IP 白名单（CIDR 命中）
        if !rec.allowed_ips.is_empty() {
            let allowed = client_ip
                .map(|ip| rec.allowed_ips.iter().any(|net| net.contains(ip)))
                .unwrap_or(false);
            if !allowed {
                return Err(LoaderError::ApiKeyIpDenied);
            }
        }

        // 后置写：last_used 失败只 warn，不影响认证结果
        if let Err(e) = self
            .api_keys
            .touch_used(rec.api_key_id, now, client_ip)
            .await
        {
            tracing::warn!(error = %e, key_id = %rec.api_key_id, "touch_used failed");
        }

        Ok(AuthContext::api_key(
            rec.api_key_id,
            rec.project_id,
            rec.org_id,
        ))
    }
}
