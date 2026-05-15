//! /v1/auth/sso — OIDC/SSO 单点登录端到端
//!
//! ```text
//! GET  /v1/auth/sso/{slug}/start?redirect_to=...   平台级 IdP 启动登录
//! GET  /v1/auth/sso/callback?code=...&state=...    回调，JIT 创建/绑定用户后签发 token
//! ```
//!
//! ## 关键不变式
//! - `state` 是 32 字节随机串；DB 里只存其 SHA-256 哈希；回调消费即删（防重放）。
//! - `client_secret_enc` 用 envelope encryption 加密；解密走 `AppState::crypto`。
//!   AAD 绑定 `provider_id`，防密文移植。
//! - JIT provisioning 仅在 `auto_create_users=true` 时启用；
//!   `email_domain_allowlist` 非空时强制邮箱命中。
//! - Org 级 IdP（`org_id` 非 NULL）+ `auto_join_org_role` 配置时，
//!   首次登录自动写 `org_memberships`（已存在则保持）。
//!
//! ## OIDC HTTP 抽象
//! `OidcClient` trait 包装 discovery + start + exchange，便于集成测试桩；
//! 生产用 [`RealOidcClient`] 走 `gate_auth::oidc::OidcProvider`。

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use async_trait::async_trait;
use axum::extract::{Path, Query, State};
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Json, Router, routing::get};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use chrono::Duration;
use chrono::{DateTime, Utc};
use gate_auth::AuthError;
use gate_auth::oidc::{OidcIdentity, OidcProvider};
use gate_core::id::UserId;
use gate_core::identity::OrgRole;
use gate_storage::{DbError, IdentityProviderRecord};
use openidconnect::{Nonce, PkceCodeVerifier};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

// ──────────────────────────────────────────────
// DTOs
// ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct StartQuery {
    /// 登录后浏览器跳转目标（可选）。Callback 完成后用 302 带回。
    pub redirect_to: Option<String>,
}

#[derive(Serialize)]
pub struct StartResponse {
    pub authorize_url: String,
    /// 给前端的状态 token；同时已落库（hash）。
    /// 前端用 cookie 持有，回调时由浏览器在 `state` query 自动带回。
    pub state: String,
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Serialize)]
pub struct CallbackResponse {
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

// ──────────────────────────────────────────────
// OIDC client 抽象（便于测试桩）
// ──────────────────────────────────────────────

/// 一次「开始登录」需要持久化的状态。与 `gate_auth::oidc::OidcStart` 同形，
/// 但用纯字符串字段，方便序列化到 DB（`PkceCodeVerifier` 等是 newtype，
/// 通过 `secret()` 拿底层串）。
pub struct StartArtifacts {
    pub authorize_url: String,
    pub csrf_state: String,
    pub pkce_verifier: String,
    pub nonce: String,
}

/// OIDC HTTP 层抽象 —— 生产用真实 `OidcProvider`，测试可注桩。
#[async_trait]
pub trait OidcClient: Send + Sync + 'static {
    async fn start(
        &self,
        idp: &IdentityProviderRecord,
        client_secret: &str,
        redirect_uri: &str,
    ) -> AppResult<StartArtifacts>;

    async fn exchange(
        &self,
        idp: &IdentityProviderRecord,
        client_secret: &str,
        redirect_uri: &str,
        code: &str,
        pkce_verifier: &str,
        nonce: &str,
    ) -> AppResult<OidcIdentity>;
}

/// 生产用 OIDC client：每次回调重新走 discovery（生产可加缓存）。
pub struct RealOidcClient;

#[async_trait]
impl OidcClient for RealOidcClient {
    async fn start(
        &self,
        idp: &IdentityProviderRecord,
        client_secret: &str,
        redirect_uri: &str,
    ) -> AppResult<StartArtifacts> {
        let provider = OidcProvider::discover(
            idp.id,
            idp.name.clone(),
            &idp.issuer,
            &idp.client_id,
            client_secret,
            redirect_uri,
            idp.scopes.clone(),
        )
        .await
        .map_err(AppError::Auth)?;

        let s = provider.start();
        Ok(StartArtifacts {
            authorize_url: s.authorize_url.into(),
            csrf_state: s.csrf_state.secret().clone(),
            pkce_verifier: s.pkce_verifier.secret().clone(),
            nonce: s.nonce.secret().clone(),
        })
    }

    async fn exchange(
        &self,
        idp: &IdentityProviderRecord,
        client_secret: &str,
        redirect_uri: &str,
        code: &str,
        pkce_verifier: &str,
        nonce: &str,
    ) -> AppResult<OidcIdentity> {
        let provider = OidcProvider::discover(
            idp.id,
            idp.name.clone(),
            &idp.issuer,
            &idp.client_id,
            client_secret,
            redirect_uri,
            idp.scopes.clone(),
        )
        .await
        .map_err(AppError::Auth)?;

        provider
            .exchange(
                code,
                PkceCodeVerifier::new(pkce_verifier.to_string()),
                &Nonce::new(nonce.to_string()),
            )
            .await
            .map_err(AppError::Auth)
    }
}

// ──────────────────────────────────────────────
// Router
// ──────────────────────────────────────────────

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/sso/:slug/start", get(start))
        .route("/auth/sso/callback", get(callback))
}

// ──────────────────────────────────────────────
// Handlers
// ──────────────────────────────────────────────

const STATE_TTL_MIN: i64 = 10;
const STATE_BYTES: usize = 32;

/// 平台级 IdP 启动登录。Org 级的 SSO start 走带 `X-Kooix-Org` 的同一路由
/// （未来扩展），目前先支持平台级。
async fn start(
    State(app): State<AppState>,
    Path(slug): Path<String>,
    Query(q): Query<StartQuery>,
) -> AppResult<Json<StartResponse>> {
    let idp = app
        .repos
        .identity_providers
        .find_platform_by_slug(&slug)
        .await
        .map_err(map_idp_err)?;

    if !idp.enabled {
        return Err(AppError::NotFound);
    }

    let client_secret = decrypt_client_secret(&app, &idp).await?;
    let redirect_uri = callback_redirect_uri(&app);

    let oidc = oidc_client(&app);
    let art = oidc.start(&idp, &client_secret, &redirect_uri).await?;

    // 生成本地不透明 state token，hash 后落库
    let state_token = generate_state_token();
    let hash = sha256_hex(&state_token);

    app.repos
        .oidc_states
        .save(
            &hash,
            idp.id,
            &art.pkce_verifier,
            &art.nonce,
            q.redirect_to.as_deref(),
            Duration::minutes(STATE_TTL_MIN),
        )
        .await?;

    // 把本地 state token 拼到 authorize_url 里——这样回调时浏览器会带回我们认得的 state
    let authorize_url = append_state(&art.authorize_url, &state_token);

    Ok(Json(StartResponse {
        authorize_url,
        state: state_token,
    }))
}

/// OIDC 回调：消费 state → 解 IdP → 验 token → JIT 用户 → 签发 JWT。
///
/// 行为分支：
/// - state 找不到/已用 → 401 token_invalid
/// - email allowlist 不命中 → 403 forbidden
/// - 非 auto_create_users 且没找到对应 user → 403 user_provisioning_disabled
/// - 成功：JSON 返回 token；若有 redirect_to，302 带 fragment
async fn callback(
    State(app): State<AppState>,
    Query(q): Query<CallbackQuery>,
) -> AppResult<Response> {
    if q.state.len() < 16 {
        return Err(AppError::Auth(AuthError::TokenInvalid(
            "state too short".into(),
        )));
    }

    let hash = sha256_hex(&q.state);
    let state_rec = app
        .repos
        .oidc_states
        .consume(&hash)
        .await
        .map_err(|e| match e {
            DbError::NotFound => AppError::Auth(AuthError::TokenInvalid("unknown state".into())),
            other => AppError::Db(other),
        })?;

    if state_rec.is_expired(Utc::now()) {
        return Err(AppError::Auth(AuthError::TokenInvalid(
            "state expired".into(),
        )));
    }

    let idp = app
        .repos
        .identity_providers
        .find_by_id(state_rec.provider_id)
        .await
        .map_err(map_idp_err)?;

    if !idp.enabled {
        return Err(AppError::NotFound);
    }

    let client_secret = decrypt_client_secret(&app, &idp).await?;
    let redirect_uri = callback_redirect_uri(&app);

    let oidc = oidc_client(&app);
    let identity = oidc
        .exchange(
            &idp,
            &client_secret,
            &redirect_uri,
            &q.code,
            &state_rec.pkce_verifier,
            &state_rec.nonce,
        )
        .await?;

    // 邮箱域 allowlist
    if !idp.email_domain_allowlist.is_empty() {
        let email = identity.email.as_deref().ok_or_else(|| {
            AppError::Auth(AuthError::Forbidden {
                action: "sso_login".into(),
                resource: "email_domain_allowlist".into(),
            })
        })?;
        let domain = email.rsplit_once('@').map(|(_, d)| d.to_lowercase());
        let allowed = domain.is_some_and(|d| {
            idp.email_domain_allowlist
                .iter()
                .any(|a| a.eq_ignore_ascii_case(&d))
        });
        if !allowed {
            return Err(AppError::Auth(AuthError::Forbidden {
                action: "sso_login".into(),
                resource: format!("email_domain:{}", email),
            }));
        }
    }

    // JIT 解析 user
    let user_id = resolve_user(&app, &idp, &identity).await?;

    // 写/更新身份绑定
    app.repos
        .user_identities
        .link(
            user_id,
            idp.id,
            &identity.subject,
            identity.email.as_deref(),
            identity.raw_claims.clone(),
        )
        .await?;

    // Org 级 IdP + auto_join_org_role：首次登录写 membership
    if let (Some(org_id), Some(role_str)) = (idp.org_id, idp.auto_join_org_role.as_deref()) {
        let role = parse_role(role_str);
        // memberships UPSERT —— 重复登录无副作用
        app.repos
            .memberships
            .add_org_member(org_id, user_id, role)
            .await?;
    }

    // 签发 JWT
    let session_id = Uuid::now_v7();
    let jti = Uuid::now_v7();

    let current_org = idp.org_id;
    let (access_token, expires_at) = app.jwt.issue_access(
        *user_id.as_uuid(),
        session_id,
        current_org.map(|o| *o.as_uuid()),
        false,
    )?;
    let (refresh_token, _) = app.jwt.issue_refresh(*user_id.as_uuid(), session_id, jti)?;

    // 顺手 mark_last_login（容错，不阻塞登录）
    app.repos
        .users
        .mark_last_login(user_id, Utc::now(), None)
        .await
        .ok();

    let user = app.repos.users.find_by_id(user_id).await?;

    // 审计：SSO 登录成功
    let audit_ctx = gate_auth::AuthContext::user(
        user_id,
        session_id,
        Default::default(),
        Default::default(),
        None,
        current_org,
    );
    app.audit.emit(
        &audit_ctx,
        "user.sso_login",
        "user",
        Some(*user_id.as_uuid()),
        Some(serde_json::json!({"idp_id": idp.id.to_string(), "idp_slug": &idp.slug})),
    );

    let body = CallbackResponse {
        access_token: access_token.clone(),
        refresh_token: refresh_token.clone(),
        expires_at,
        user: UserSummary {
            id: user.id.to_string(),
            email: user.email,
            display_name: user.display_name,
        },
    };

    // 有 redirect_to → 302 把 token 通过 fragment 带回（避免落 referer/log）
    if let Some(target) = state_rec.redirect_to {
        let url = format!(
            "{target}#access_token={access_token}&refresh_token={refresh_token}&expires_at={}",
            urlencoding::encode_owned(expires_at.to_rfc3339())
        );
        let mut resp = Redirect::temporary(&url).into_response();
        // 显式标注 cache-control，避免代理缓存 token
        resp.headers_mut()
            .insert("cache-control", HeaderValue::from_static("no-store"));
        return Ok(resp);
    }

    Ok(Json(body).into_response())
}

// ──────────────────────────────────────────────
// 内部工具
// ──────────────────────────────────────────────

fn map_idp_err(e: DbError) -> AppError {
    match e {
        DbError::NotFound => AppError::NotFound,
        other => AppError::Db(other),
    }
}

fn callback_redirect_uri(app: &AppState) -> String {
    // 走 AppState 上挂的 public_origin；测试场景被 OidcClient 桩 mock 时不真用，
    // 默认拼一个 placeholder（用于 RealOidcClient 时必须由部署方覆盖）。
    let origin = app
        .public_origin
        .as_deref()
        .unwrap_or("http://localhost:8080");
    format!("{origin}/v1/auth/sso/callback")
}

fn oidc_client(app: &AppState) -> Arc<dyn OidcClient> {
    if let Some(c) = app.oidc_client.as_ref() {
        return c.clone();
    }
    Arc::new(RealOidcClient)
}

async fn decrypt_client_secret(app: &AppState, idp: &IdentityProviderRecord) -> AppResult<String> {
    let crypto = app
        .crypto
        .as_ref()
        .ok_or_else(|| AppError::Internal("crypto KMS not configured".into()))?;
    let aad = gate_crypto::aad::idp_secret(idp.id);
    let pt = crypto
        .open(&idp.client_secret_enc, &aad)
        .await
        .map_err(|e| AppError::Internal(format!("client_secret decrypt: {e}")))?;
    String::from_utf8(pt.to_vec()).map_err(|_| AppError::Internal("client_secret not utf-8".into()))
}

async fn resolve_user(
    app: &AppState,
    idp: &IdentityProviderRecord,
    identity: &OidcIdentity,
) -> AppResult<UserId> {
    // 1. 已绑定？
    if let Some(rec) = app
        .repos
        .user_identities
        .find_by_provider_subject(idp.id, &identity.subject)
        .await?
    {
        return Ok(rec.user_id);
    }

    // 2. 同邮箱用户？绑过去（避免重复账户）
    if let Some(email) = identity.email.as_deref() {
        match app.repos.users.find_by_email(email).await {
            Ok(u) => return Ok(u.id),
            Err(DbError::NotFound) => {}
            Err(other) => return Err(AppError::Db(other)),
        }
    }

    // 3. 自动创建？
    if !idp.auto_create_users {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "sso_login".into(),
            resource: "user_provisioning_disabled".into(),
        }));
    }

    let email = identity.email.as_deref().ok_or_else(|| {
        AppError::Auth(AuthError::Forbidden {
            action: "sso_login".into(),
            resource: "missing_email_claim".into(),
        })
    })?;

    let display = identity.name.as_deref();
    let user = app.repos.users.create(email, None, display).await?;
    Ok(user.id)
}

fn parse_role(s: &str) -> OrgRole {
    match s {
        "owner" => OrgRole::Owner,
        "admin" => OrgRole::Admin,
        "billing_viewer" => OrgRole::BillingViewer,
        _ => OrgRole::Member,
    }
}

fn generate_state_token() -> String {
    let mut buf = [0u8; STATE_BYTES];
    rand::thread_rng().fill_bytes(&mut buf);
    B64.encode(buf)
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}

/// authorize_url 默认带 IdP 给的 state；我们把它改成自家的不透明 token，
/// 这样回调里直接用 `state` 串做 hash 查表。
fn append_state(url: &str, token: &str) -> String {
    // 先去掉 IdP 自带的 state= 段，再追加我们自己的
    let mut clean: Vec<&str> = url
        .split_once('?')
        .map(|(_, q)| q)
        .unwrap_or("")
        .split('&')
        .filter(|kv| !kv.starts_with("state="))
        .collect();
    let new_state = format!("state={}", urlencoding::encode_owned(token.to_string()));
    clean.push(&new_state);
    let base = url.split_once('?').map(|(b, _)| b).unwrap_or(url);
    format!("{base}?{}", clean.join("&"))
}

// ──────────────────────────────────────────────
// AppState 扩展位：OIDC client 注入（测试用）
// ──────────────────────────────────────────────

// 直接 hook 到 state 模块 — 测试时通过 with_oidc_client 注入桩。
mod state_ext {
    use super::OidcClient;
    use crate::state::AppState;
    use std::sync::Arc;

    impl AppState {
        /// 注入自定义 OIDC HTTP 客户端（测试用）。
        pub fn with_oidc_client(mut self, c: Arc<dyn OidcClient>) -> Self {
            self.oidc_client = Some(c);
            self
        }
    }
}

// 极简 URL encoder —— 不引新依赖。仅 percent-encode 危险字符。
mod urlencoding {
    pub fn encode_owned(s: String) -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{:02X}", b),
            })
            .collect()
    }
}
