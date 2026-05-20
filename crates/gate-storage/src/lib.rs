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
pub use repo::audit::{AuditRecord, AuditRepo, InMemoryAuditRepo, PgAuditRepo};
pub use repo::billing::{
    BillingInvoice, BillingRepo, BillingSeed, InMemoryBillingRepo, InvoiceStatus, ModelBillLine,
    MonthlyBill, PgBillingRepo, ProjectBillLine, UsageExportRow,
};
pub use repo::channel::{
    ChannelBinding, ChannelGroupRecord, ChannelGroupRepo, ChannelRecord, ChannelRepo,
    ChannelStatus, CreateChannel, InMemoryChannelGroupRepo, InMemoryChannelRepo, ListChannelsQuery,
    PaginatedChannels, PgChannelGroupRepo, PgChannelRepo, UpdateChannel, UpdateChannelBinding,
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
    ModelRank, PgRequestLogRepo, RequestFilter, RequestLogRepo, RequestPage, RequestRecord,
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

pub async fn connect(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(url)
        .await
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    migrator().run(pool).await
}

/// 取得编译期内嵌的迁移器。kgctl 用它做 dry-run / 版本号读取。
pub fn migrator() -> sqlx::migrate::Migrator {
    sqlx::migrate!("./migrations")
}
