//! gate-storage: PostgreSQL 持久层
//!
//! Repository pattern — 每个领域一个 Repo trait，sqlx 实现。
//! 错误经 [`DbError`] 统一收口，调用方区分 NotFound / Conflict / Internal。

pub mod error;
pub mod migrations;
pub mod repo;
pub mod rls;

pub use error::{DbError, DbResult};
pub use repo::api_key::{ApiKeyRecord, ApiKeyRepo, ApiKeySummaryRecord, PgApiKeyRepo};
pub use repo::audit::{
    AuditRecord, AuditRepo, AuditSortBy, InMemoryAuditRepo, PgAuditRepo, SortDirection,
};
pub use repo::billing::{
    BillingInvoice, BillingRepo, BillingSeed, InMemoryBillingRepo, InvoiceStatus, ModelBillLine,
    MonthlyBill, PgBillingRepo, ProjectBillLine, UsageExportRow,
};
pub use repo::channel::{
    ChannelBinding, ChannelGroupRecord, ChannelGroupRepo, ChannelRecord, ChannelRepo,
    ChannelStatus, CreateChannel, InMemoryChannelGroupRepo, InMemoryChannelRepo, ListChannelsQuery,
    PaginatedChannels, PgChannelGroupRepo, PgChannelRepo, UpdateChannel, UpdateChannelBinding,
};
pub use repo::channel_health_score::{
    ChannelHealthScore, ChannelHealthScoreRepo, HealthState, InMemoryChannelHealthScoreRepo,
    OutcomeObservation, PgChannelHealthScoreRepo, ScoreUpdate,
};
pub use repo::channel_key::{
    ChannelKeyRecord, ChannelKeyRepo, InMemoryChannelKeyRepo, PgChannelKeyRepo,
};
pub use repo::channel_latency::{
    ChannelLatencyRepo, InMemoryChannelLatencyRepo, PgChannelLatencyRepo,
};
pub use repo::identity::{
    IdentityProviderCreate, IdentityProviderRecord, IdentityProviderRepo, IdentityProviderUpdate,
    InMemoryIdentityProviderRepo, InMemoryOidcStateRepo, InMemoryUserIdentityRepo, OidcStateRecord,
    OidcStateRepo, PgIdentityProviderRepo, PgOidcStateRepo, PgUserIdentityRepo, UserIdentityRecord,
    UserIdentityRepo,
};
pub use repo::inflight::{
    ExpiredInFlight, InFlightRecord, InFlightRepo, InMemoryInFlightRepo, PgInFlightRepo,
};
pub use repo::invitation::{
    InMemoryInvitationRepo, InvitationCreate, InvitationRecord, InvitationRepo, PgInvitationRepo,
};
pub use repo::membership::{MembershipRepo, OrgMemberView, PgMembershipRepo, UserMemberships};
pub use repo::memory::{
    InMemoryApiKeyRepo, InMemoryMembershipRepo, InMemoryOrgRepo, InMemoryProjectRepo,
    InMemoryUserRepo,
};
pub use repo::model_alias::{
    InMemoryModelAliasRepo, ModelAliasRecord, ModelAliasRepo, PgModelAliasRepo, ResolvedAlias,
};
pub use repo::org::{OrgRepo, PgOrgRepo};
pub use repo::project::{PgProjectRepo, ProjectRepo};
pub use repo::quota::{InMemoryQuotaRepo, PgQuotaRepo, QuotaRecord, QuotaRepo, QuotaUpsert};
pub use repo::request_log::{
    DashboardStats, FilterOptionItem, FilterOptions, HourlyBucket, InMemoryRequestLogRepo,
    IncidentSummary, ModelRank, PgRequestLogRepo, RequestFilter, RequestLogRepo, RequestPage,
    RequestRecord, TopFailingChannel, UpstreamErrorClasses,
};
pub use repo::session::{
    InMemoryUserSessionRepo, PgUserSessionRepo, UserSessionCreate, UserSessionRecord,
    UserSessionRepo,
};
pub use repo::usage::{
    GroupBy as UsageGroupBy, InMemoryUsageRepo, PgUsageRepo, ScopeUsageFilter, UsageBucket,
    UsageRepo, UsageSeed, UsageTotals,
};
pub use repo::user::{PgUserRepo, UserRepo};
pub use rls::RlsContext;
pub use sqlx::PgPool;

/// 0.4.71（product-review §1.5）：PostgreSQL pool 配置。
///
/// 之前 `connect(url, max)` 硬编码 acquire_timeout=5s，没暴露 min_connections /
/// idle_timeout / max_lifetime —— 在生产负载下，pool 长时间空连接会被云厂商
/// LB（如 RDS）回收，下次拿连接 acquire 就可能超时；缺 min_connections 也意味
/// 着冷启动后第一波突发流量需排队等连接。
///
/// 默认值（`KOOIX_DB_*` 未设时）：
/// - max_connections = 20（远高于原 16，配合多 worker 部署）
/// - min_connections = 2（warm pool）
/// - acquire_timeout = 3s（小于原 5s，更快暴露 pool 紧张问题）
/// - idle_timeout = 600s（10min，多数云厂商 LB 默认 5-15min）
/// - max_lifetime = 1800s（30min，强制回收防止长连接累积内存）
///
/// 生产可通过 env 覆盖：
/// - `KOOIX_DB_MAX_CONNECTIONS`
/// - `KOOIX_DB_MIN_CONNECTIONS`
/// - `KOOIX_DB_ACQUIRE_TIMEOUT_SECS`
/// - `KOOIX_DB_IDLE_TIMEOUT_SECS`
/// - `KOOIX_DB_MAX_LIFETIME_SECS`
#[derive(Debug, Clone, Copy)]
pub struct PoolConfig {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout_secs: u64,
    pub idle_timeout_secs: Option<u64>,
    pub max_lifetime_secs: Option<u64>,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 20,
            min_connections: 2,
            acquire_timeout_secs: 3,
            idle_timeout_secs: Some(600),
            max_lifetime_secs: Some(1800),
        }
    }
}

impl PoolConfig {
    /// 从 env 读 KOOIX_DB_* 覆盖默认值。
    /// 未设置或 parse 失败的项保留 default。
    pub fn from_env() -> Self {
        fn env_u32(name: &str) -> Option<u32> {
            std::env::var(name).ok().and_then(|s| s.parse().ok())
        }
        fn env_u64(name: &str) -> Option<u64> {
            std::env::var(name).ok().and_then(|s| s.parse().ok())
        }
        fn env_opt_u64(name: &str) -> Option<Option<u64>> {
            std::env::var(name).ok().map(|s| {
                if s == "0" || s.is_empty() {
                    None
                } else {
                    s.parse().ok()
                }
            })
        }
        let mut cfg = Self::default();
        if let Some(v) = env_u32("KOOIX_DB_MAX_CONNECTIONS") {
            cfg.max_connections = v.max(1);
        }
        if let Some(v) = env_u32("KOOIX_DB_MIN_CONNECTIONS") {
            cfg.min_connections = v.min(cfg.max_connections);
        }
        if let Some(v) = env_u64("KOOIX_DB_ACQUIRE_TIMEOUT_SECS") {
            cfg.acquire_timeout_secs = v.max(1);
        }
        if let Some(v) = env_opt_u64("KOOIX_DB_IDLE_TIMEOUT_SECS") {
            cfg.idle_timeout_secs = v;
        }
        if let Some(v) = env_opt_u64("KOOIX_DB_MAX_LIFETIME_SECS") {
            cfg.max_lifetime_secs = v;
        }
        cfg
    }
}

pub async fn connect(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    // 0.4.71: 向后兼容 caller —— 保留旧签名，max_connections 覆盖 PoolConfig.max
    let mut cfg = PoolConfig::from_env();
    cfg.max_connections = max_connections;
    cfg.min_connections = cfg.min_connections.min(max_connections);
    connect_with_config(url, &cfg).await
}

/// 0.4.71: 用完整 PoolConfig 建池。生产推荐用此 API。
pub async fn connect_with_config(url: &str, cfg: &PoolConfig) -> Result<PgPool, sqlx::Error> {
    let mut opts = sqlx::postgres::PgPoolOptions::new()
        .max_connections(cfg.max_connections)
        .min_connections(cfg.min_connections)
        .acquire_timeout(std::time::Duration::from_secs(cfg.acquire_timeout_secs));
    if let Some(s) = cfg.idle_timeout_secs {
        opts = opts.idle_timeout(std::time::Duration::from_secs(s));
    }
    if let Some(s) = cfg.max_lifetime_secs {
        opts = opts.max_lifetime(std::time::Duration::from_secs(s));
    }
    opts.connect(url).await
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    migrator().run(pool).await
}

/// 取得编译期内嵌的迁移器。kgctl 用它做 dry-run / 版本号读取。
pub fn migrator() -> sqlx::migrate::Migrator {
    sqlx::migrate!("./migrations")
}

#[cfg(test)]
mod pool_config_tests {
    use super::*;

    /// PoolConfig::from_env 内涉及 std::env::var，这些测试是 best-effort 的：
    /// 不并发跑（默认 cargo test 共享进程），且都本地 set+remove。
    fn with_env<F>(pairs: &[(&str, &str)], f: F)
    where
        F: FnOnce(),
    {
        let prev: Vec<(String, Option<String>)> = pairs
            .iter()
            .map(|(k, _)| ((*k).to_string(), std::env::var(*k).ok()))
            .collect();
        for (k, v) in pairs {
            // SAFETY: 测试单线程，env 操作可控
            unsafe { std::env::set_var(k, v) };
        }
        f();
        for (k, prev_v) in prev {
            match prev_v {
                Some(v) => unsafe { std::env::set_var(&k, v) },
                None => unsafe { std::env::remove_var(&k) },
            }
        }
    }

    #[test]
    fn default_pool_config_is_safe() {
        let cfg = PoolConfig::default();
        assert!(cfg.max_connections >= 4);
        assert!(cfg.min_connections <= cfg.max_connections);
        assert!(cfg.acquire_timeout_secs >= 1);
        assert!(cfg.idle_timeout_secs.is_some());
        assert!(cfg.max_lifetime_secs.is_some());
    }

    #[test]
    fn env_override_max_connections() {
        with_env(&[("KOOIX_DB_MAX_CONNECTIONS", "50")], || {
            let cfg = PoolConfig::from_env();
            assert_eq!(cfg.max_connections, 50);
        });
    }

    #[test]
    fn env_min_connections_capped_by_max() {
        with_env(
            &[
                ("KOOIX_DB_MAX_CONNECTIONS", "10"),
                ("KOOIX_DB_MIN_CONNECTIONS", "999"),
            ],
            || {
                let cfg = PoolConfig::from_env();
                assert_eq!(cfg.max_connections, 10);
                assert_eq!(cfg.min_connections, 10, "min must be capped by max");
            },
        );
    }

    #[test]
    fn env_idle_timeout_zero_disables() {
        with_env(&[("KOOIX_DB_IDLE_TIMEOUT_SECS", "0")], || {
            let cfg = PoolConfig::from_env();
            assert!(cfg.idle_timeout_secs.is_none());
        });
    }

    #[test]
    fn env_bogus_values_fall_back_to_default() {
        with_env(&[("KOOIX_DB_ACQUIRE_TIMEOUT_SECS", "not-a-number")], || {
            let cfg = PoolConfig::from_env();
            assert_eq!(cfg.acquire_timeout_secs, 3, "default kept on parse failure");
        });
    }
}
