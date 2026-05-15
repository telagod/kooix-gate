//! D2 集成测试：SSO/OIDC 端到端
//!
//! 策略：用 `OidcClient` trait stub 替代真实 IdP HTTP，测试覆盖
//! - start：成功生成 authorize_url + 落库 state（一次性消费）
//! - callback：JIT 创建用户 + 签发 token
//! - 未知 slug → 404
//! - email_domain_allowlist 不命中 → 403
//! - state 长度不足 → 401
//! - state 一次性消费（重放无效）
//! - auto_create_users=false 且邮箱无匹配 → 403
//! - Org 级 IdP + auto_join_org_role → 自动写 org_memberships

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_auth::oidc::OidcIdentity;
use gate_core::id::*;
use gate_crypto::{EnvKms, EnvelopeKms, Sealer, kms::generate_master_key_b64};
use gate_server::loader::InMemoryLoader;
use gate_server::routes::sso::{OidcClient, StartArtifacts};
use gate_server::state::Repos;
use gate_server::{AppState, build_router};
use gate_storage::{
    IdentityProviderRecord, InMemoryApiKeyRepo, InMemoryChannelGroupRepo, InMemoryChannelKeyRepo,
    InMemoryChannelRepo, InMemoryIdentityProviderRepo, InMemoryMembershipRepo,
    InMemoryOidcStateRepo, InMemoryOrgRepo, InMemoryProjectRepo, InMemoryUserIdentityRepo,
    InMemoryUserRepo, MembershipRepo, OidcStateRepo, UserIdentityRepo, UserRepo,
};
use http_body_util::BodyExt;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use uuid::Uuid;

// ──────────────────────────────────────────────
// Stub OidcClient
// ──────────────────────────────────────────────

#[derive(Default)]
struct StubOidc {
    /// 期望返回的 identity（按 code 索引；缺省返回 default identity）
    identity: Mutex<Option<OidcIdentity>>,
    /// 记录 start/exchange 调用次数
    pub calls: Mutex<Vec<&'static str>>,
}

impl StubOidc {
    fn new(identity: OidcIdentity) -> Arc<Self> {
        Arc::new(Self {
            identity: Mutex::new(Some(identity)),
            calls: Mutex::new(vec![]),
        })
    }
}

#[async_trait]
impl OidcClient for StubOidc {
    async fn start(
        &self,
        idp: &IdentityProviderRecord,
        _client_secret: &str,
        redirect_uri: &str,
    ) -> Result<StartArtifacts, gate_server::AppError> {
        self.calls.lock().unwrap().push("start");
        Ok(StartArtifacts {
            authorize_url: format!(
                "https://idp.example.com/auth?client_id={}&redirect_uri={}&state=ignored",
                idp.client_id, redirect_uri
            ),
            csrf_state: "stub-csrf".into(),
            pkce_verifier: "stub-pkce-verifier-aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            nonce: "stub-nonce".into(),
        })
    }

    async fn exchange(
        &self,
        _idp: &IdentityProviderRecord,
        _client_secret: &str,
        _redirect_uri: &str,
        _code: &str,
        _pkce_verifier: &str,
        _nonce: &str,
    ) -> Result<OidcIdentity, gate_server::AppError> {
        self.calls.lock().unwrap().push("exchange");
        Ok(self.identity.lock().unwrap().clone().unwrap())
    }
}

// ──────────────────────────────────────────────
// Fixture
// ──────────────────────────────────────────────

struct Fixture {
    router: axum::Router,
    users: Arc<dyn UserRepo>,
    user_identities: Arc<dyn UserIdentityRepo>,
    oidc_states: Arc<dyn OidcStateRepo>,
    memberships: Arc<dyn MembershipRepo>,
    idp_id: Uuid,
}

async fn build_fixture(
    identity: OidcIdentity,
    mut idp: IdentityProviderRecord,
) -> (Fixture, Arc<StubOidc>) {
    let kms = EnvKms::from_b64(&generate_master_key_b64(), "t").unwrap();
    let sealer: EnvelopeKms = Sealer::new(kms);

    // 提前把 client_secret 塞 envelope 加密 —— 模拟真实部署
    let aad = gate_crypto::aad::idp_secret(idp.id);
    let ct = sealer.seal(b"super-secret-client-pw", &aad).await.unwrap();
    idp.client_secret_enc = ct;

    // 直接构造内存 repos —— 避免 dyn 下的 downcast seed 难题
    let users: Arc<dyn UserRepo> = Arc::new(InMemoryUserRepo::new());
    let memberships: Arc<dyn MembershipRepo> = Arc::new(InMemoryMembershipRepo::new());
    let user_identities: Arc<dyn UserIdentityRepo> = Arc::new(InMemoryUserIdentityRepo::new());
    let oidc_states: Arc<dyn OidcStateRepo> = Arc::new(InMemoryOidcStateRepo::new());

    let idp_repo_concrete = Arc::new(InMemoryIdentityProviderRepo::new());
    idp_repo_concrete.seed(idp.clone());

    let repos = Repos {
        users: users.clone(),
        orgs: Arc::new(InMemoryOrgRepo::new()),
        projects: Arc::new(InMemoryProjectRepo::new()),
        memberships: memberships.clone(),
        api_keys: Arc::new(InMemoryApiKeyRepo::new()),
        channels: Arc::new(InMemoryChannelRepo::new()),
        channel_groups: Arc::new(InMemoryChannelGroupRepo::new()),
        channel_keys: Arc::new(InMemoryChannelKeyRepo::new()),
        identity_providers: idp_repo_concrete.clone(),
        user_identities: user_identities.clone(),
        oidc_states: oidc_states.clone(),
        usage: Arc::new(gate_storage::InMemoryUsageRepo::new()),
        quotas: Arc::new(gate_storage::InMemoryQuotaRepo::new()),
        model_aliases: Arc::new(gate_storage::InMemoryModelAliasRepo::new()),
        audit: Arc::new(gate_storage::InMemoryAuditRepo::new()),
        billing: Arc::new(gate_storage::InMemoryBillingRepo::new()),
        request_logs: Arc::new(gate_storage::InMemoryRequestLogRepo::new()),
        inflight: Arc::new(gate_storage::InMemoryInFlightRepo::new()),
        pg_pool: None,
    };

    let jwt = JwtIssuer::new(
        b"test-secret-32-bytes-minimum-ok!",
        "kg-test",
        "console",
        TokenLifetimes {
            access: ChronoDuration::minutes(15),
            refresh: ChronoDuration::days(1),
        },
    )
    .unwrap();

    let stub = StubOidc::new(identity);
    let loader = Arc::new(InMemoryLoader::new());
    let state = AppState::new(jwt, loader, repos)
        .with_crypto(sealer)
        .with_oidc_client(stub.clone() as Arc<dyn OidcClient>)
        .with_public_origin("https://gate.test");

    let router = build_router(state);

    (
        Fixture {
            router,
            users,
            user_identities,
            oidc_states,
            memberships,
            idp_id: idp.id,
        },
        stub,
    )
}

fn sample_idp(org_id: Option<OrgId>) -> IdentityProviderRecord {
    IdentityProviderRecord {
        id: Uuid::now_v7(),
        org_id,
        name: "Stub IdP".into(),
        slug: "stub".into(),
        issuer: "https://idp.example.com".into(),
        client_id: "client-abc".into(),
        client_secret_enc: vec![],
        scopes: vec!["openid".into(), "email".into(), "profile".into()],
        email_claim: "email".into(),
        name_claim: "name".into(),
        subject_claim: "sub".into(),
        auto_create_users: true,
        auto_join_org_role: None,
        email_domain_allowlist: vec![],
        enabled: true,
    }
}

fn sample_identity(email: &str, sub: &str) -> OidcIdentity {
    OidcIdentity {
        subject: sub.into(),
        email: Some(email.into()),
        email_verified: true,
        name: Some("Test User".into()),
        raw_claims: serde_json::json!({"sub": sub, "email": email}),
    }
}

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

async fn get(router: &axum::Router, uri: &str) -> (StatusCode, Value, axum::http::HeaderMap) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body, headers)
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[tokio::test]
async fn start_returns_authorize_url_and_persists_state() {
    let idp = sample_idp(None);
    let (f, stub) = build_fixture(sample_identity("a@b.c", "sub-1"), idp.clone()).await;

    let (status, body, _) = get(&f.router, "/v1/auth/sso/stub/start?redirect_to=/back").await;
    assert_eq!(status, StatusCode::OK, "body={body}");

    let url = body["authorize_url"].as_str().unwrap();
    let state = body["state"].as_str().unwrap();
    assert!(url.contains("client_id=client-abc"));
    assert!(state.len() >= 32);

    // 调用记录
    {
        let calls = stub.calls.lock().unwrap();
        assert_eq!(*calls, vec!["start"]);
    }

    // state 已落库（hash），可被 consume
    let hash = sha256_hex(state);
    let rec = f.oidc_states.consume(&hash).await.unwrap();
    assert_eq!(rec.provider_id, f.idp_id);
    assert_eq!(rec.redirect_to.as_deref(), Some("/back"));
}

#[tokio::test]
async fn start_unknown_slug_returns_404() {
    let idp = sample_idp(None);
    let (f, _) = build_fixture(sample_identity("a@b.c", "sub-1"), idp).await;

    let (status, body, _) = get(&f.router, "/v1/auth/sso/no-such-idp/start").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body={body}");
}

#[tokio::test]
async fn callback_jit_creates_user_and_issues_token() {
    let idp = sample_idp(None);
    let (f, _) = build_fixture(sample_identity("alice@example.com", "sub-alice"), idp).await;

    // 1. start
    let (_, body, _) = get(&f.router, "/v1/auth/sso/stub/start").await;
    let state = body["state"].as_str().unwrap();

    // 2. callback
    let url = format!("/v1/auth/sso/callback?code=code-1&state={state}");
    let (status, body, _) = get(&f.router, &url).await;
    assert_eq!(status, StatusCode::OK, "body={body}");

    let access = body["access_token"].as_str().unwrap();
    assert!(!access.is_empty());
    assert!(!body["refresh_token"].as_str().unwrap().is_empty());
    assert_eq!(body["user"]["email"], "alice@example.com");

    // user_identities 已绑定
    let bound = f
        .user_identities
        .find_by_provider_subject(f.idp_id, "sub-alice")
        .await
        .unwrap();
    assert!(bound.is_some());
}

#[tokio::test]
async fn callback_email_not_in_allowlist_returns_403() {
    let mut idp = sample_idp(None);
    idp.email_domain_allowlist = vec!["good.com".into()];
    let (f, _) = build_fixture(sample_identity("alice@evil.com", "sub-evil"), idp).await;

    let (_, body, _) = get(&f.router, "/v1/auth/sso/stub/start").await;
    let state = body["state"].as_str().unwrap();

    let url = format!("/v1/auth/sso/callback?code=code-1&state={state}");
    let (status, body, _) = get(&f.router, &url).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn callback_replay_state_rejected() {
    let idp = sample_idp(None);
    let (f, _) = build_fixture(sample_identity("alice@example.com", "sub-alice"), idp).await;

    let (_, body, _) = get(&f.router, "/v1/auth/sso/stub/start").await;
    let state = body["state"].as_str().unwrap().to_string();

    // 第一次成功
    let url = format!("/v1/auth/sso/callback?code=code-1&state={state}");
    let (status, _, _) = get(&f.router, &url).await;
    assert_eq!(status, StatusCode::OK);

    // 重放：state 已被消费 → 401
    let (status, body, _) = get(&f.router, &url).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body={body}");
    assert_eq!(body["error"]["code"], "token_invalid");
}

#[tokio::test]
async fn callback_short_state_rejected() {
    let idp = sample_idp(None);
    let (f, _) = build_fixture(sample_identity("a@b.c", "sub-x"), idp).await;

    let (status, body, _) = get(&f.router, "/v1/auth/sso/callback?code=c&state=short").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body={body}");
    assert_eq!(body["error"]["code"], "token_invalid");
}

#[tokio::test]
async fn callback_no_auto_provisioning_returns_403() {
    let mut idp = sample_idp(None);
    idp.auto_create_users = false;
    let (f, _) = build_fixture(sample_identity("brandnew@x.com", "sub-new"), idp).await;

    let (_, body, _) = get(&f.router, "/v1/auth/sso/stub/start").await;
    let state = body["state"].as_str().unwrap();

    let url = format!("/v1/auth/sso/callback?code=code-1&state={state}");
    let (status, body, _) = get(&f.router, &url).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn start_is_public_no_auth_required() {
    let idp = sample_idp(None);
    let (f, _) = build_fixture(sample_identity("a@b.c", "sub-1"), idp).await;

    // 不带 Authorization header
    let req = Request::builder()
        .method("GET")
        .uri("/v1/auth/sso/stub/start")
        .body(Body::empty())
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn callback_links_existing_user_by_email() {
    let idp = sample_idp(None);
    let (f, _) = build_fixture(sample_identity("alice@example.com", "sub-fresh"), idp).await;

    // 先用 password repo 创建 alice
    let alice = f
        .users
        .create("alice@example.com", Some("argon2-fake"), Some("Alice"))
        .await
        .unwrap();

    let (_, body, _) = get(&f.router, "/v1/auth/sso/stub/start").await;
    let state = body["state"].as_str().unwrap();

    let url = format!("/v1/auth/sso/callback?code=c&state={state}");
    let (status, body, _) = get(&f.router, &url).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(body["user"]["email"], "alice@example.com");

    // 应该绑定到现有 alice，而不是新建一个
    let bound = f
        .user_identities
        .find_by_provider_subject(f.idp_id, "sub-fresh")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bound.user_id, alice.id);
}

#[tokio::test]
async fn callback_redirect_to_emits_302_with_fragment() {
    let idp = sample_idp(None);
    let (f, _) = build_fixture(sample_identity("a@b.c", "sub-redir"), idp).await;

    let (_, body, _) = get(
        &f.router,
        "/v1/auth/sso/stub/start?redirect_to=https://app.test/done",
    )
    .await;
    let state = body["state"].as_str().unwrap();

    let url = format!("/v1/auth/sso/callback?code=c&state={state}");
    let req = Request::builder()
        .method("GET")
        .uri(&url)
        .body(Body::empty())
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    let loc = resp.headers().get("location").unwrap().to_str().unwrap();
    assert!(loc.starts_with("https://app.test/done#access_token="));
    assert!(loc.contains("&refresh_token="));
    // 防缓存
    assert_eq!(resp.headers().get("cache-control").unwrap(), "no-store");
}

#[tokio::test]
async fn allowlist_passes_when_email_matches() {
    let mut idp = sample_idp(None);
    idp.email_domain_allowlist = vec!["acme.com".into()];
    let (f, _) = build_fixture(sample_identity("bob@ACME.com", "sub-bob"), idp).await;

    let (_, body, _) = get(&f.router, "/v1/auth/sso/stub/start").await;
    let state = body["state"].as_str().unwrap();

    let url = format!("/v1/auth/sso/callback?code=c&state={state}");
    let (status, _, _) = get(&f.router, &url).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "case-insensitive domain should pass"
    );
}

#[tokio::test]
async fn org_level_idp_membership_is_added_via_repo() {
    // Org 级 IdP 当前 start 路由（平台级）找不到，所以本测试直接验
    // 调用 callback 路径会写 org_memberships —— 通过手动写入 oidc_login_states
    // 模拟一次「假装 start 已完成」的状态。
    let org_id = OrgId::new();
    let mut idp = sample_idp(Some(org_id));
    idp.auto_join_org_role = Some("member".into());
    let (f, _) = build_fixture(sample_identity("emp@corp.com", "sub-emp"), idp).await;

    // 准备 user
    let alice = f
        .users
        .create("emp@corp.com", None, Some("Emp"))
        .await
        .unwrap();
    // 直接绑定 identity，避免走 JIT 创建分支
    f.user_identities
        .link(
            alice.id,
            f.idp_id,
            "sub-emp",
            Some("emp@corp.com"),
            serde_json::json!({}),
        )
        .await
        .unwrap();

    // 直接 save 一条 state 记录，绕过 platform-only start 路由
    let state_token = "orglevel-state-aaaaaaaaaaaaaaaaaaaa".to_string();
    let hash = sha256_hex(&state_token);
    f.oidc_states
        .save(
            &hash,
            f.idp_id,
            "stub-pkce-verifier-aaaaaaaaaaaaaaaaaaaaaaaa",
            "stub-nonce",
            None,
            ChronoDuration::minutes(5),
        )
        .await
        .unwrap();

    let url = format!("/v1/auth/sso/callback?code=c&state={state_token}");
    let (status, body, _) = get(&f.router, &url).await;
    assert_eq!(status, StatusCode::OK, "body={body}");

    // 校验 membership 写入
    let m = f.memberships.load_for_user(alice.id).await.unwrap();
    assert!(m.orgs.contains_key(&org_id), "membership should be added");
}

// ──────────────────────────────────────────────
// 内部 helper
// ──────────────────────────────────────────────

fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}
