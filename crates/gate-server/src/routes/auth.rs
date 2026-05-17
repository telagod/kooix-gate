//! /v1/auth — 登录 / 刷新 / 登出
//!
//! POST /v1/auth/login   — 邮箱密码登录，签发 access + refresh token
//! POST /v1/auth/refresh — 用 refresh token 换新 access token
//! POST /v1/auth/logout  — 当前 session 登出（暂返回 ok，待接 session 撤销表）

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::State;
use axum::{Json, Router, routing::post};
use chrono::{DateTime, Utc};
use gate_auth::AuthError;
use gate_storage::DbError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ──────────────────────────────────────────────
// DTOs
// ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub user: UserSummary,
}

#[derive(Serialize)]
pub struct UserSummary {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Serialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
}

// ──────────────────────────────────────────────
// Router
// ──────────────────────────────────────────────

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
}

// ──────────────────────────────────────────────
// Handlers
// ──────────────────────────────────────────────

/// POST /v1/auth/login
///
/// 失败统一返 401 `invalid_credentials`，不区分"邮箱不存在"与"密码错"。
/// 连续失败 >= 5 次额外返 423 `too_many_failures`。
async fn login(
    State(app): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    // 1. 查凭证 — NotFound 也映射成 invalid_credentials
    let (user, pw_hash) =
        app.repos
            .users
            .find_credentials(&req.email)
            .await
            .map_err(|e| match e {
                DbError::NotFound => AppError::Auth(AuthError::InvalidCredentials),
                other => AppError::Db(other),
            })?;

    // 2. 非 active 用户不可登录；返回统一 suspended 错误，不签发任何 token。
    if !matches!(user.status, gate_core::identity::UserStatus::Active) {
        return Err(AppError::Auth(AuthError::AccountSuspended));
    }

    // 3. SSO 用户没有密码 → invalid_credentials
    let Some(hash) = pw_hash else {
        return Err(AppError::Auth(AuthError::InvalidCredentials));
    };

    // 4. 校验密码
    if gate_auth::password::verify(&req.password, &hash).is_err() {
        // 登录失败 +1，超过 5 次给 too_many_failures
        let count = app
            .repos
            .users
            .bump_failed_login(user.id)
            .await
            .unwrap_or(0);
        if count >= 5 {
            return Err(AppError::Auth(AuthError::TooManyFailures));
        }
        return Err(AppError::Auth(AuthError::InvalidCredentials));
    }

    // 5. 登录成功 — 重置失败计数 + mark_last_login
    app.repos.users.reset_failed_login(user.id).await.ok();
    app.repos
        .users
        .mark_last_login(user.id, Utc::now(), None)
        .await
        .ok();

    // 6. 签发 token 对
    let session_id = Uuid::now_v7();
    let jti = Uuid::now_v7();

    let (access_token, expires_at) =
        app.jwt
            .issue_access(*user.id.as_uuid(), session_id, None, false)?;

    let (refresh_token, _) = app.jwt.issue_refresh(*user.id.as_uuid(), session_id, jti)?;

    // 审计：登录成功
    let audit_ctx = gate_auth::AuthContext::user(
        user.id,
        session_id,
        Default::default(),
        Default::default(),
        None,
        None,
    );
    app.audit.emit(
        &audit_ctx,
        "user.login",
        "user",
        Some(*user.id.as_uuid()),
        Some(serde_json::json!({"method": "password"})),
    );

    Ok(Json(LoginResponse {
        access_token,
        refresh_token,
        expires_at,
        user: UserSummary {
            id: user.id.to_string(),
            email: user.email,
            display_name: user.display_name,
        },
    }))
}

/// POST /v1/auth/refresh
///
/// refresh token 无效/过期 → 401 `token_invalid`。
async fn refresh(
    State(app): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> AppResult<Json<RefreshResponse>> {
    let claims = app
        .jwt
        .parse_refresh(&req.refresh_token)
        .map_err(|e| match e {
            AuthError::TokenExpired => {
                AppError::Auth(AuthError::TokenInvalid("refresh token expired".into()))
            }
            other => AppError::Auth(other),
        })?;

    let user = app
        .repos
        .users
        .find_by_id(gate_core::id::UserId::from(claims.sub))
        .await
        .map_err(|e| match e {
            DbError::NotFound => AppError::Auth(AuthError::InvalidCredentials),
            other => AppError::Db(other),
        })?;
    if !matches!(user.status, gate_core::identity::UserStatus::Active) {
        return Err(AppError::Auth(AuthError::AccountSuspended));
    }

    // 签发新 access token，session_id 继承
    let (access_token, expires_at) = app.jwt.issue_access(claims.sub, claims.sid, None, false)?;

    Ok(Json(RefreshResponse {
        access_token,
        expires_at,
    }))
}

/// POST /v1/auth/logout
///
/// 当前实现：无状态，仅返回 ok。
// TODO: 接 session 撤销表（Redis 黑名单 / user_sessions.revoked_at）
async fn logout(_authed: Authed) -> AppResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({ "ok": true })))
}
