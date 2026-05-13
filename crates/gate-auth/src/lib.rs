//! gate-auth: 认证与授权门面
//!
//! 模块分工：
//! - [`password`] argon2id 密码哈希/校验
//! - [`jwt`]      控制台 access + refresh token
//! - [`api_key`]  API Key 生成 / hash / 校验（调用方凭证）
//! - [`oidc`]     OIDC 客户端（PKCE + 状态校验）
//! - [`context`]  `AuthContext` —— 一次性解析好的「我能干什么」
//! - [`permit`]   `require!` 宏 —— 强制权限检查门面
//!
//! Axum 抽取器留给 gate-server 层接入，不在这里耦合 HTTP。

pub mod error;
pub mod password;
pub mod jwt;
pub mod api_key;
pub mod oidc;
pub mod context;
pub mod permit;

pub use context::{AuthContext, Subject};
pub use error::{AuthError, Result};
