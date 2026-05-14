//! AppState: 跨 handler 共享的依赖
//!
//! 用 Arc 实现 Clone 廉价化。

use crate::audit::AuditEmitter;
use crate::loader::AuthContextLoader;
use gate_auth::jwt::JwtIssuer;
use gate_billing::{OutboxRepo, PricingRepo};
use gate_cache::{QuotaCounter, RateLimiter};
use gate_crypto::EnvelopeKms;
use gate_providers::{Provider, ProviderRouter};
use gate_storage::{
    ApiKeyRepo, AuditRepo, BillingRepo, ChannelGroupRepo, ChannelKeyRepo, ChannelRepo,
    IdentityProviderRepo, MembershipRepo, ModelAliasRepo, OidcStateRepo, OrgRepo, ProjectRepo,
    QuotaRepo, RequestLogRepo, UsageRepo, UserIdentityRepo, UserRepo,
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
    /// Envelope KMS：解密 `client_secret_enc` 等机密。SSO 路径必填，未配置则
    /// 回调阶段返 500，避免误用未加密 secret。
    pub crypto: Option<Arc<EnvelopeKms>>,
    /// OIDC HTTP 客户端注入位。生产为 None（走默认 RealOidcClient），
    /// 测试时通过 `with_oidc_client` 注桩绕开真实 IdP。
    pub oidc_client: Option<Arc<dyn crate::routes::sso::OidcClient>>,
    /// 公网入口前缀，用于拼 OIDC redirect_uri 等绝对 URL（e.g. `https://gate.example.com`）。
    /// 未配置时回落到 `http://localhost:8080`（仅 dev 用）。
    pub public_origin: Option<String>,
    /// 计费 outbox：handler 把 UsageEvent 推到这里，由 Consumer 异步落 usage_records。
    /// 未配置时 chat handler 不计费（warn-only，不阻断请求）。
    pub outbox: Option<Arc<dyn OutboxRepo>>,
    /// 计费定价表：handler 拿 (channel_id, model, at) 查单价用。
    /// 未配置时 chat handler 不计费（warn-only，不阻断请求）。
    pub pricing: Option<Arc<dyn PricingRepo>>,
    /// 配额预扣/查询计数器（Redis 后端）。
    /// 未配置时 quota middleware 对 budget 维度走 fail-open，仅 rate 维度仍可生效（走 rate_limiter）。
    pub quota_counter: Option<Arc<QuotaCounter>>,
    /// 审计发射器（G2 新增）。用 repos.audit 初始化，非阻塞写入。
    pub audit: AuditEmitter,
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
    /// Channel key 池（G1 新增，加密 API key 存储/轮转）
    pub channel_keys: Arc<dyn ChannelKeyRepo>,
    /// SSO/OIDC 相关（D2 追加）。
    pub identity_providers: Arc<dyn IdentityProviderRepo>,
    pub user_identities: Arc<dyn UserIdentityRepo>,
    pub oidc_states: Arc<dyn OidcStateRepo>,
    /// 用量聚合（E2 追加）—— 控制台 /v1/usage 仪表盘读它。
    pub usage: Arc<dyn UsageRepo>,
    /// 配额定义（E3 追加）—— quota_enforce middleware 用它筛选生效 quota。
    pub quotas: Arc<dyn QuotaRepo>,
    /// 模型别名（F5 追加）—— router 做 alias → target_model 解析。
    pub model_aliases: Arc<dyn ModelAliasRepo>,
    /// 审计日志（G2 追加）—— 关键操作落 audit_logs 表。
    pub audit: Arc<dyn AuditRepo>,
    /// 计费聚合（G3 追加）—— 月度账单 + CSV 导出。
    pub billing: Arc<dyn BillingRepo>,
    /// 请求日志（H1 追加）—— 逐条请求记录 + Dashboard 聚合。
    pub request_logs: Arc<dyn RequestLogRepo>,
    #[doc(hidden)]
    pub pg_pool: Option<sqlx::PgPool>,
}

impl Repos {
    /// 从一个 PgPool 批量构造全部 Pg 实现。
    pub fn from_pg(pool: sqlx::PgPool) -> Self {
        use gate_storage::{
            PgApiKeyRepo, PgAuditRepo, PgBillingRepo, PgChannelGroupRepo, PgChannelKeyRepo,
            PgChannelRepo, PgIdentityProviderRepo, PgMembershipRepo, PgModelAliasRepo,
            PgOidcStateRepo, PgOrgRepo, PgProjectRepo, PgQuotaRepo, PgRequestLogRepo, PgUsageRepo,
            PgUserIdentityRepo, PgUserRepo,
        };
        Self {
            users: Arc::new(PgUserRepo::new(pool.clone())),
            orgs: Arc::new(PgOrgRepo::new(pool.clone())),
            projects: Arc::new(PgProjectRepo::new(pool.clone())),
            memberships: Arc::new(PgMembershipRepo::new(pool.clone())),
            api_keys: Arc::new(PgApiKeyRepo::new(pool.clone())),
            channels: Arc::new(PgChannelRepo::new(pool.clone())),
            channel_groups: Arc::new(PgChannelGroupRepo::new(pool.clone())),
            channel_keys: Arc::new(PgChannelKeyRepo::new(pool.clone())),
            identity_providers: Arc::new(PgIdentityProviderRepo::new(pool.clone())),
            user_identities: Arc::new(PgUserIdentityRepo::new(pool.clone())),
            oidc_states: Arc::new(PgOidcStateRepo::new(pool.clone())),
            usage: Arc::new(PgUsageRepo::new(pool.clone())),
            quotas: Arc::new(PgQuotaRepo::new(pool.clone())),
            model_aliases: Arc::new(PgModelAliasRepo::new(pool.clone())),
            audit: Arc::new(PgAuditRepo::new(pool.clone())),
            billing: Arc::new(PgBillingRepo::new(pool.clone())),
            request_logs: Arc::new(PgRequestLogRepo::new(pool.clone())),
            pg_pool: Some(pool),
        }
    }

    /// 内存版（dev 模式 / 测试用）。
    pub fn in_memory() -> Self {
        use gate_storage::{
            InMemoryApiKeyRepo, InMemoryAuditRepo, InMemoryBillingRepo, InMemoryChannelGroupRepo,
            InMemoryChannelKeyRepo, InMemoryChannelRepo, InMemoryIdentityProviderRepo,
            InMemoryMembershipRepo, InMemoryModelAliasRepo, InMemoryOidcStateRepo,
            InMemoryOrgRepo, InMemoryProjectRepo, InMemoryQuotaRepo, InMemoryRequestLogRepo,
            InMemoryUsageRepo, InMemoryUserIdentityRepo, InMemoryUserRepo,
        };
        Self {
            users: Arc::new(InMemoryUserRepo::new()),
            orgs: Arc::new(InMemoryOrgRepo::new()),
            projects: Arc::new(InMemoryProjectRepo::new()),
            memberships: Arc::new(InMemoryMembershipRepo::new()),
            api_keys: Arc::new(InMemoryApiKeyRepo::new()),
            channels: Arc::new(InMemoryChannelRepo::new()),
            channel_groups: Arc::new(InMemoryChannelGroupRepo::new()),
            channel_keys: Arc::new(InMemoryChannelKeyRepo::new()),
            identity_providers: Arc::new(InMemoryIdentityProviderRepo::new()),
            user_identities: Arc::new(InMemoryUserIdentityRepo::new()),
            oidc_states: Arc::new(InMemoryOidcStateRepo::new()),
            usage: Arc::new(InMemoryUsageRepo::new()),
            quotas: Arc::new(InMemoryQuotaRepo::new()),
            model_aliases: Arc::new(InMemoryModelAliasRepo::new()),
            audit: Arc::new(InMemoryAuditRepo::new()),
            billing: Arc::new(InMemoryBillingRepo::new()),
            request_logs: Arc::new(InMemoryRequestLogRepo::new()),
            pg_pool: None,
        }
    }

    pub fn pool(&self) -> Option<&sqlx::PgPool> {
        self.pg_pool.as_ref()
    }
}

impl AppState {
    pub fn new(jwt: JwtIssuer, loader: Arc<dyn AuthContextLoader>, repos: Repos) -> Self {
        let audit = AuditEmitter::new(repos.audit.clone());
        Self {
            jwt: Arc::new(jwt),
            loader,
            repos,
            rate_limiter: None,
            rate_limit_cfg: RateLimitCfg::default(),
            provider: None,
            provider_router: None,
            crypto: None,
            oidc_client: None,
            public_origin: None,
            outbox: None,
            pricing: None,
            quota_counter: None,
            audit,
        }
    }

    /// 带限流器构造（生产路径）。
    pub fn with_rate_limiter(mut self, rl: RateLimiter) -> Self {
        self.rate_limiter = Some(Arc::new(rl));
        self
    }

    /// 带限流器构造（已经 Arc 包装）。
    pub fn with_rate_limiter_arc(mut self, rl: Arc<RateLimiter>) -> Self {
        self.rate_limiter = Some(rl);
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

    /// 挂载 Envelope KMS（SSO 回调解密 client_secret）。
    pub fn with_crypto(mut self, kms: EnvelopeKms) -> Self {
        self.crypto = Some(Arc::new(kms));
        self
    }

    /// 挂载 Envelope KMS（已 Arc 包装）。
    pub fn with_crypto_arc(mut self, kms: Arc<EnvelopeKms>) -> Self {
        self.crypto = Some(kms);
        self
    }

    /// 设置公网入口前缀（用于 OIDC redirect_uri 等绝对 URL）。
    pub fn with_public_origin(mut self, origin: impl Into<String>) -> Self {
        self.public_origin = Some(origin.into());
        self
    }

    /// 挂载计费 outbox（D4 新增）。未挂载时计费走 warn-only 路径。
    pub fn with_outbox(mut self, outbox: Arc<dyn OutboxRepo>) -> Self {
        self.outbox = Some(outbox);
        self
    }

    /// 挂载计费 pricing 仓库（D4 新增）。未挂载时计费走 warn-only 路径。
    pub fn with_pricing(mut self, pricing: Arc<dyn PricingRepo>) -> Self {
        self.pricing = Some(pricing);
        self
    }

    /// 挂载 quota 计数器（E3 新增）。未挂载时 budget 配额检查走 fail-open。
    pub fn with_quota_counter(mut self, qc: QuotaCounter) -> Self {
        self.quota_counter = Some(Arc::new(qc));
        self
    }
}
