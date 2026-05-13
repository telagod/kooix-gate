//! gate-billing: 计费 outbox 消费层。
//!
//! 主要组件：
//! - [`UsageEvent`] — 用量事件结构，与 usage_records 表对齐
//! - [`OutboxRepo`] — outbox_events 表读写 trait
//! - [`PgOutboxRepo`] / [`InMemoryOutboxRepo`] — 持久 / 内存实现
//! - [`PricingRepo`] — model_pricing 查询 trait
//! - [`PgPricingRepo`] / [`InMemoryPricingRepo`] — 持久 / 内存实现
//! - [`compute_cost_micros`] — usage × pricing → cost_micros
//! - [`Consumer`] — 消费循环：拉批 → commit_usage → mark_done

pub mod consumer;
pub mod outbox;
pub mod pricing;
pub mod types;

pub use consumer::Consumer;
pub use outbox::{InMemoryOutboxRepo, OutboxRepo, PgOutboxRepo};
pub use pricing::{
    InMemoryPricingRepo, ModelPricing, PgPricingRepo, PricingRepo, compute_cost_micros,
};
pub use types::UsageEvent;

#[derive(Debug, thiserror::Error)]
pub enum BillingError {
    #[error("storage error: {0}")]
    Storage(#[from] gate_storage::DbError),
    #[error("db error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("internal: {0}")]
    Internal(String),
}

pub type BillingResult<T> = Result<T, BillingError>;
