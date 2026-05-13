//! gate-storage: PostgreSQL 持久层
//!
//! Repository pattern — 每个领域一个 Repo trait，sqlx 实现。

pub mod migrations;

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
