//! Axum middleware 组合。
//!
//! - [`base`] 基础：request_id / trace
//! - [`rate_limit`] 基于 gate-cache 的滑窗限流，按 AuthContext 区分 subject

pub mod base;
pub mod rate_limit;

pub use base::{request_id_layers, trace_layer};
pub use rate_limit::rate_limit_by_subject;
