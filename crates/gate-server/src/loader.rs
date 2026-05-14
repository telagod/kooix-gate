//! AuthContextLoader: 从 user_id / api_key_hash 解析 AuthContext 的数据源
//!
//! 真实实现走 gate-storage 查 PG（待办）。
//! 本模块提供 trait + InMemoryLoader stub —— 让 gate-server 能脱离 DB 编译/测试。

use async_trait::async_trait;
use gate_auth::AuthContext;
use gate_core::id::*;
use gate_core::identity::{OrgRole, PlatformRole, ProjectRole};
use std::collections::HashMap;
use parking_lot::RwLock;

/// 从用户 ID 加载完整角色快照；从 api_key 明文加载 ApiKey 上下文。
///
/// 实现者负责：
/// - User: 查 org_memberships / project_memberships / platform_admins
/// - ApiKey: hash 明文 → 查 api_keys 表 → 校验未撤销/未过期/IP/模型范围
#[async_trait]
pub trait AuthContextLoader: Send + Sync + 'static {
    /// JWT 校验成功后调用，把 (user_id, current_org) 解析成完整 AuthContext
    async fn load_user(
        &self,
        user_id: UserId,
        session_id: uuid::Uuid,
        current_org: Option<OrgId>,
    ) -> Result<AuthContext, LoaderError>;

    /// API key 明文校验。返回 AuthContext + 必要的 ApiKey 元信息（用于配额检查、IP 白名单等）
    async fn load_api_key(
        &self,
        plaintext: &str,
        client_ip: Option<std::net::IpAddr>,
    ) -> Result<AuthContext, LoaderError>;
}

#[derive(Debug, thiserror::Error)]
pub enum LoaderError {
    #[error("user not found or suspended")]
    UserUnavailable,
    #[error("api key invalid")]
    ApiKeyInvalid,
    #[error("api key revoked")]
    ApiKeyRevoked,
    #[error("api key ip not allowed")]
    ApiKeyIpDenied,
    #[error("internal: {0}")]
    Internal(String),
}

// ----------------------------------------------------------------------------
// In-memory implementation 用于测试 / 早期 demo
// ----------------------------------------------------------------------------

#[derive(Default)]
pub struct InMemoryLoader {
    inner: RwLock<Inner>,
}

#[derive(Default)]
struct Inner {
    users: HashMap<UserId, UserRecord>,
    api_keys: HashMap<String, ApiKeyRecord>, // key = hex sha256(plaintext)
}

#[derive(Clone)]
pub struct UserRecord {
    pub orgs: HashMap<OrgId, OrgRole>,
    pub projects: HashMap<(OrgId, ProjectId), ProjectRole>,
    pub platform: Option<PlatformRole>,
}

#[derive(Clone)]
pub struct ApiKeyRecord {
    pub api_key_id: ApiKeyId,
    pub project_id: ProjectId,
    pub org_id: OrgId,
    pub revoked: bool,
    pub allowed_ips: Vec<std::net::IpAddr>,
}

impl InMemoryLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_user(&self, id: UserId, record: UserRecord) {
        self.inner.write().users.insert(id, record);
    }

    /// 用明文 API key 注册（自动计算 hash）
    pub fn add_api_key(&self, plaintext: &str, record: ApiKeyRecord) {
        let h = gate_auth::api_key::hash(plaintext);
        self.inner.write().api_keys.insert(h, record);
    }
}

#[async_trait]
impl AuthContextLoader for InMemoryLoader {
    async fn load_user(
        &self,
        user_id: UserId,
        session_id: uuid::Uuid,
        current_org: Option<OrgId>,
    ) -> Result<AuthContext, LoaderError> {
        let inner = self.inner.read();
        let rec = inner
            .users
            .get(&user_id)
            .ok_or(LoaderError::UserUnavailable)?;
        Ok(AuthContext::user(
            user_id,
            session_id,
            rec.orgs.clone(),
            rec.projects.clone(),
            rec.platform,
            current_org,
        ))
    }

    async fn load_api_key(
        &self,
        plaintext: &str,
        client_ip: Option<std::net::IpAddr>,
    ) -> Result<AuthContext, LoaderError> {
        let hash = gate_auth::api_key::hash(plaintext);
        let inner = self.inner.read();
        let rec = inner
            .api_keys
            .get(&hash)
            .ok_or(LoaderError::ApiKeyInvalid)?;
        if rec.revoked {
            return Err(LoaderError::ApiKeyRevoked);
        }
        if !rec.allowed_ips.is_empty() {
            match client_ip {
                Some(ip) if rec.allowed_ips.contains(&ip) => {}
                _ => return Err(LoaderError::ApiKeyIpDenied),
            }
        }
        Ok(AuthContext::api_key(
            rec.api_key_id,
            rec.project_id,
            rec.org_id,
        ))
    }
}
