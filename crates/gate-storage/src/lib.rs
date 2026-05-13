//! gate-storage: PostgreSQL 持久层
//!
//! Repository pattern — 每个领域一个 Repo trait，sqlx 实现。
//! 错误经 [`DbError`] 统一收口，调用方区分 NotFound / Conflict / Internal。

pub mod error;
pub mod migrations;
pub mod repo;

pub use error::{DbError, DbResult};
pub use repo::api_key::{ApiKeyRecord, ApiKeyRepo, PgApiKeyRepo};
pub use repo::channel::{
    ChannelBinding, ChannelGroupRecord, ChannelGroupRepo, ChannelRecord, ChannelRepo,
    InMemoryChannelGroupRepo, InMemoryChannelRepo, PgChannelGroupRepo, PgChannelRepo,
};
pub use repo::identity::{
    IdentityProviderRecord, IdentityProviderRepo, InMemoryIdentityProviderRepo,
    InMemoryOidcStateRepo, InMemoryUserIdentityRepo, OidcStateRecord, OidcStateRepo,
    PgIdentityProviderRepo, PgOidcStateRepo, PgUserIdentityRepo, UserIdentityRecord,
    UserIdentityRepo,
};
pub use repo::membership::{MembershipRepo, PgMembershipRepo, UserMemberships};
pub use repo::memory::{
    InMemoryApiKeyRepo, InMemoryMembershipRepo, InMemoryOrgRepo, InMemoryProjectRepo,
    InMemoryUserRepo,
};
pub use repo::org::{OrgRepo, PgOrgRepo};
pub use repo::project::{PgProjectRepo, ProjectRepo};
pub use repo::user::{PgUserRepo, UserRepo};
pub use sqlx::PgPool;

pub async fn connect(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(url)
        .await
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
