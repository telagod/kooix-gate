//! gate-billing: 多维度计费引擎 + outbox 消费层。
//!
//! 主要组件：
//! - [`PricingRule`] — 多维度定价规则
//! - [`CostContext`] — 一次请求的全部用量维度
//! - [`compute_cost`] — rules × context → cost_micros
//! - [`PricingRepo`] — pricing_rules 查询/CRUD trait
//! - [`PgPricingRepo`] / [`InMemoryPricingRepo`] — 持久 / 内存实现
//! - [`compute_cost_micros`] — Legacy compat (Usage × ModelPricing)
//! - [`UsageEvent`] — 用量事件结构
//! - [`OutboxRepo`] — outbox_events 表读写 trait
//! - [`Consumer`] — 消费循环

pub mod consumer;
pub mod outbox;
pub mod pricing;
pub mod pricing_sync;
pub mod types;

pub use consumer::Consumer;
pub use outbox::{InMemoryOutboxRepo, OutboxRepo, PgOutboxRepo};
pub use pricing::{
    CostContext, InMemoryPricingRepo, ModelPricing, PgPricingRepo, PricingRepo, PricingRule,
    compute_cost, compute_cost_micros,
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
