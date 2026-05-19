//! Axum middleware 组合。
//!
//! - [`base`] 基础：request_id / trace
//! - [`rate_limit`] 基于 gate-cache 的滑窗限流，按 AuthContext 区分 subject
//! - [`quota`] 基于 quotas 表的多维度配额执行（rpm/tpm/daily_budget）
//! - [`rls`] RLS 上下文注入：把 AuthContext 转为 RlsContext 写入 extensions
//! - [`metrics`] HTTP 请求计数 + 耗时采集（Prometheus 后端）

pub mod base;
pub mod metrics;
pub mod quota;
pub mod rate_limit;
pub mod rls;

pub use base::{KooixRequestId, request_id_extension, request_id_layers, trace_layer};
pub use metrics::metrics_layer;
pub use quota::quota_enforce;
pub use rate_limit::rate_limit_by_subject;
pub use rls::rls_inject;
