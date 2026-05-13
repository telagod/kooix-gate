//! Auth 抽取器：把 Authorization header 解析成 AuthContext
//!
//! 识别规则：
//! - `sk-kg-*` / `Bearer sk-kg-*` → API key 路径
//! - `Bearer <jwt>` → 控制台用户路径
//! - 缺失或其他 → anonymous（handler 自行决定是否拒绝）
//!
//! 可选 Org 上下文：请求头 `X-Kooix-Org: <org-uuid>` 用于切换激活租户。
//! - User：header 存在必须属于该 user 的 memberships（本模块不校验，由 require! 后置）
//! - API key：忽略 header，强制用 ApiKey 绑定的 Org

use crate::error::AppError;
use crate::state::AppState;
use async_trait::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::{header, request::Parts};
use gate_auth::{AuthContext, AuthError};
use gate_core::id::OrgId;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

/// 强制抽取 AuthContext — 未认证直接 401。
pub struct Authed(pub AuthContext);

/// 可选抽取 — 未认证给 anonymous，handler 自己判断。
pub struct MaybeAuthed(pub AuthContext);

#[async_trait]
impl<S> FromRequestParts<S> for Authed
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let ctx = resolve(parts, state).await?;
        if ctx.subject().is_none() {
            return Err(AppError::Auth(AuthError::MissingCredentials));
        }
        Ok(Self(ctx))
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for MaybeAuthed
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(resolve(parts, state).await?))
    }
}

async fn resolve<S>(parts: &mut Parts, state: &S) -> Result<AuthContext, AppError>
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    let app: AppState = AppState::from_ref(state);

    let auth_header = parts
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let Some(raw) = auth_header else {
        return Ok(AuthContext::anonymous());
    };

    // --- API key 路径 ---
    if let Some(plain) = gate_auth::api_key::extract_from_header(&raw) {
        let client_ip = extract_client_ip(parts);
        let ctx = app.loader.load_api_key(plain, client_ip).await?;
        return Ok(ctx);
    }

    // --- JWT 路径 ---
    let bearer = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))
        .ok_or(AppError::Auth(AuthError::TokenInvalid(
            "unknown authorization scheme".into(),
        )))?;

    let claims = app.jwt.parse_access(bearer)?;

    // X-Kooix-Org 可显式切换激活 Org；否则用 JWT 里的 org
    let header_org = parts
        .headers
        .get("x-kooix-org")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .map(OrgId::from);

    let current_org = header_org.or_else(|| claims.org.map(OrgId::from));

    let ctx = app
        .loader
        .load_user(claims.sub.into(), claims.sid, current_org)
        .await?;

    // 防越权：header 指定的 Org 必须在用户可访问列表中（SuperAdmin 例外）
    if let Some(req_org) = header_org
        && !ctx.is_super_admin()
        && !ctx.accessible_orgs().contains(&req_org)
    {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "switch_org".into(),
            resource: format!("org:{req_org}"),
        }));
    }

    Ok(ctx)
}

/// 从 ConnectInfo / X-Forwarded-For / X-Real-IP 抽客户端 IP。
/// 反向代理场景：Nginx 需要配 `proxy_set_header X-Forwarded-For $remote_addr`。
fn extract_client_ip(parts: &Parts) -> Option<IpAddr> {
    // X-Forwarded-For 取第一个
    if let Some(v) = parts
        .headers
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        && let Some(first) = v.split(',').next().map(str::trim)
        && let Ok(ip) = first.parse::<IpAddr>()
    {
        return Some(ip);
    }
    if let Some(v) = parts.headers.get("x-real-ip").and_then(|h| h.to_str().ok())
        && let Ok(ip) = v.trim().parse::<IpAddr>()
    {
        return Some(ip);
    }
    // axum ConnectInfo
    parts
        .extensions
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
}

/// 辅助：让 AppState 可作为 FromRef 的直接 source
impl FromRef<AppState> for Arc<dyn crate::loader::AuthContextLoader> {
    fn from_ref(s: &AppState) -> Self {
        s.loader.clone()
    }
}
