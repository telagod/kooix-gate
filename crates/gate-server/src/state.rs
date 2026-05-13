//! AppState: 跨 handler 共享的依赖
//!
//! 用 Arc 实现 Clone 廉价化。

use crate::loader::AuthContextLoader;
use gate_auth::jwt::JwtIssuer;
use gate_storage::{ApiKeyRepo, MembershipRepo, OrgRepo, ProjectRepo, UserRepo};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub jwt: Arc<JwtIssuer>,
    pub loader: Arc<dyn AuthContextLoader>,
    pub repos: Repos,
}

/// 业务 handler 用到的所有 Repo 聚合。
///
/// 独立成 struct 是为了：
/// 1. 未来加 Repo 不用改 AppState 签名
/// 2. 测试可以只注入需要的那几个（其他填默认桩）
#[derive(Clone)]
pub struct Repos {
    pub users: Arc<dyn UserRepo>,
    pub orgs: Arc<dyn OrgRepo>,
    pub projects: Arc<dyn ProjectRepo>,
    pub memberships: Arc<dyn MembershipRepo>,
    pub api_keys: Arc<dyn ApiKeyRepo>,
}

impl Repos {
    /// 从一个 PgPool 批量构造全部 Pg 实现。
    pub fn from_pg(pool: sqlx::PgPool) -> Self {
        use gate_storage::{
            PgApiKeyRepo, PgMembershipRepo, PgOrgRepo, PgProjectRepo, PgUserRepo,
        };
        Self {
            users: Arc::new(PgUserRepo::new(pool.clone())),
            orgs: Arc::new(PgOrgRepo::new(pool.clone())),
            projects: Arc::new(PgProjectRepo::new(pool.clone())),
            memberships: Arc::new(PgMembershipRepo::new(pool.clone())),
            api_keys: Arc::new(PgApiKeyRepo::new(pool)),
        }
    }

    /// 内存版（dev 模式 / 测试用）。
    pub fn in_memory() -> Self {
        use gate_storage::{
            InMemoryApiKeyRepo, InMemoryMembershipRepo, InMemoryOrgRepo, InMemoryProjectRepo,
            InMemoryUserRepo,
        };
        Self {
            users: Arc::new(InMemoryUserRepo::new()),
            orgs: Arc::new(InMemoryOrgRepo::new()),
            projects: Arc::new(InMemoryProjectRepo::new()),
            memberships: Arc::new(InMemoryMembershipRepo::new()),
            api_keys: Arc::new(InMemoryApiKeyRepo::new()),
        }
    }
}

impl AppState {
    pub fn new(
        jwt: JwtIssuer,
        loader: Arc<dyn AuthContextLoader>,
        repos: Repos,
    ) -> Self {
        Self {
            jwt: Arc::new(jwt),
            loader,
            repos,
        }
    }
}
