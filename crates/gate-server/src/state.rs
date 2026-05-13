//! AppState: 跨 handler 共享的依赖
//!
//! 用 Arc 实现 Clone 廉价化。

use crate::loader::AuthContextLoader;
use gate_auth::jwt::JwtIssuer;
use gate_cache::RateLimiter;
use gate_providers::{Provider, ProviderRouter};
use gate_storage::{
    ApiKeyRepo, ChannelGroupRepo, ChannelRepo, MembershipRepo, OrgRepo, ProjectRepo, UserRepo,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub jwt: Arc<JwtIssuer>,
    pub loader: Arc<dyn AuthContextLoader>,
    pub repos: Repos,
    /// 可选的限流器：未配置 Redis 时为 None，middleware 走 fail-open。
    pub rate_limiter: Option<Arc<RateLimiter>>,
    pub rate_limit_cfg: RateLimitCfg,
    /// 默认 Provider — 现阶段单一 OpenAI 兼容。后续拓展为路由表。
    pub provider: Option<Arc<dyn Provider>>,
    /// 多 Provider 路由器（C1 新增）。优先于 provider 字段使用。
    /// 未配置时退化为 provider 字段。
    pub provider_router: Option<Arc<ProviderRouter>>,
}

/// 限流参数。可未来按 plan/api-key 维度差异化。
#[derive(Clone, Copy, Debug)]
pub struct RateLimitCfg {
    pub window_ms: u64,
    pub capacity: u64,
}

impl Default for RateLimitCfg {
    fn default() -> Self {
        Self {
            window_ms: 60_000,
            capacity: 600,
        }
    }
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
    /// Channel repos（C1 新增，路由用）
    pub channels: Arc<dyn ChannelRepo>,
    pub channel_groups: Arc<dyn ChannelGroupRepo>,
}

impl Repos {
    /// 从一个 PgPool 批量构造全部 Pg 实现。
    pub fn from_pg(pool: sqlx::PgPool) -> Self {
        use gate_storage::{
            PgApiKeyRepo, PgChannelGroupRepo, PgChannelRepo, PgMembershipRepo, PgOrgRepo,
            PgProjectRepo, PgUserRepo,
        };
        Self {
            users: Arc::new(PgUserRepo::new(pool.clone())),
            orgs: Arc::new(PgOrgRepo::new(pool.clone())),
            projects: Arc::new(PgProjectRepo::new(pool.clone())),
            memberships: Arc::new(PgMembershipRepo::new(pool.clone())),
            api_keys: Arc::new(PgApiKeyRepo::new(pool.clone())),
            channels: Arc::new(PgChannelRepo::new(pool.clone())),
            channel_groups: Arc::new(PgChannelGroupRepo::new(pool)),
        }
    }

    /// 内存版（dev 模式 / 测试用）。
    pub fn in_memory() -> Self {
        use gate_storage::{
            InMemoryApiKeyRepo, InMemoryChannelGroupRepo, InMemoryChannelRepo,
            InMemoryMembershipRepo, InMemoryOrgRepo, InMemoryProjectRepo, InMemoryUserRepo,
        };
        Self {
            users: Arc::new(InMemoryUserRepo::new()),
            orgs: Arc::new(InMemoryOrgRepo::new()),
            projects: Arc::new(InMemoryProjectRepo::new()),
            memberships: Arc::new(InMemoryMembershipRepo::new()),
            api_keys: Arc::new(InMemoryApiKeyRepo::new()),
            channels: Arc::new(InMemoryChannelRepo::new()),
            channel_groups: Arc::new(InMemoryChannelGroupRepo::new()),
        }
    }
}

impl AppState {
    pub fn new(jwt: JwtIssuer, loader: Arc<dyn AuthContextLoader>, repos: Repos) -> Self {
        Self {
            jwt: Arc::new(jwt),
            loader,
            repos,
            rate_limiter: None,
            rate_limit_cfg: RateLimitCfg::default(),
            provider: None,
            provider_router: None,
        }
    }

    /// 带限流器构造（生产路径）。
    pub fn with_rate_limiter(mut self, rl: RateLimiter) -> Self {
        self.rate_limiter = Some(Arc::new(rl));
        self
    }

    pub fn with_rate_limit_cfg(mut self, cfg: RateLimitCfg) -> Self {
        self.rate_limit_cfg = cfg;
        self
    }

    pub fn with_provider<P: Provider>(mut self, provider: P) -> Self {
        self.provider = Some(Arc::new(provider));
        self
    }

    /// 挂载 ProviderRouter（C1 新增）。
    pub fn with_provider_router(mut self, router: ProviderRouter) -> Self {
        self.provider_router = Some(Arc::new(router));
        self
    }
}
