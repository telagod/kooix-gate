//! `require!` 宏：所有授权检查的唯一入口
//!
//! 用法：
//! ```ignore
//! use gate_auth::require;
//! use gate_core::rbac::{Permission, Scope};
//!
//! async fn handler(ctx: AuthContext) -> Result<()> {
//!     require!(ctx, Permission::ApiKeyCreate, Scope::Project { org: &org, project: &proj });
//!     // ...
//!     Ok(())
//! }
//! ```
//!
//! 宏强制所有 handler 走统一接口 — code review 时只盯 `require!` 调用就够。

/// 检查权限，失败立即 `return Err(AuthError::Forbidden { ... })`。
#[macro_export]
macro_rules! require {
    ($ctx:expr, $perm:expr, $scope:expr) => {{
        $ctx.require($perm, $scope)?;
    }};
}

/// 仅判断不抛错，返回 bool。
#[macro_export]
macro_rules! can {
    ($ctx:expr, $perm:expr, $scope:expr) => {{ $ctx.can($perm, $scope) }};
}

/// 限定 subject 必须是 User（拒绝 API key 调用）—— 管理类端点必备。
#[macro_export]
macro_rules! require_user {
    ($ctx:expr) => {{
        if !$ctx.subject().map(|s| s.is_user()).unwrap_or(false) {
            return Err($crate::error::AuthError::Forbidden {
                action: "management".into(),
                resource: "api_key_subject_not_allowed".into(),
            }
            .into());
        }
    }};
}

/// 限定 subject 必须是 ApiKey（推理类端点用，拒绝控制台凭证）
#[macro_export]
macro_rules! require_api_key {
    ($ctx:expr) => {{
        if !$ctx.subject().map(|s| s.is_api_key()).unwrap_or(false) {
            return Err($crate::error::AuthError::Forbidden {
                action: "inference".into(),
                resource: "user_subject_not_allowed".into(),
            }
            .into());
        }
    }};
}
