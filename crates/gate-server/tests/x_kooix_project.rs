//! D1-#2 X-Kooix-Project: User 主体补 project 头路由 + 越权防护
//!
//! 覆盖矩阵：
//! - 合法：user 是 Project Developer + 提供合法 project header → 200，走到上游
//! - fallback：user 不传 X-Kooix-Project → 走全局 fallback provider（200）
//! - 格式错：X-Kooix-Project: not-uuid → 400
//! - 跨 Org 伪造：project 真存在但属于别的 Org → 403
//! - 无角色：project 属于 ctx.current_org，但 user 没在该 project 也没在该 Org → 403
//! - SuperAdmin 短路：传 Project A 也通（即使没角色） → 200

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use chrono::Utc;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::id::{ChannelGroupId, ChannelId, OrgId, ProjectId, UserId};
use gate_core::identity::{
    OrgRole, OrgStatus, Organization, PlatformRole, Project, ProjectRole, ProjectStatus,
};
use gate_providers::ProviderRouter;
use gate_providers::openai::OpenAiProvider;
use gate_server::loader::{InMemoryLoader, UserRecord};
use gate_server::state::Repos;
use gate_server::{AppState, build_router};
use gate_storage::{
    ChannelGroupRecord, ChannelRecord, InMemoryChannelGroupRepo, InMemoryChannelRepo,
    InMemoryProjectRepo,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Fixture {
    router: axum::Router,
    tok_dev: String,      // user_dev: Org A 的 Developer 在 proj_a
    tok_outsider: String, // user_outsider: 不属于 Org A
    tok_super: String,    // 平台 SuperAdmin
    proj_a: ProjectId,
    proj_b_other_org: ProjectId,
    org_a: OrgId,
}

async fn fixture() -> Fixture {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-xkp",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })))
        .mount(&upstream)
        .await;
    // 这个 upstream 既被 fallback provider 使用，也被 routed channel 使用 —— 都打 200。
    // 之所以这样能区分：当 user 没传 header → fallback；传了合法 header → router 选 channel；
    // 两种路径都通到同一个 mock，但测试用 status code 区分（200/400/403）。
    let upstream_uri = upstream.uri();
    Box::leak(Box::new(upstream)); // keep alive for the whole test

    let org_a = OrgId::new();
    let org_b = OrgId::new();
    let proj_a = ProjectId::new();
    let proj_b_other_org = ProjectId::new();
    let now = Utc::now();

    let projects_repo = Arc::new(InMemoryProjectRepo::new());
    projects_repo.seed(Project {
        id: proj_a,
        org_id: org_a,
        name: "main".into(),
        slug: "main".into(),
        status: ProjectStatus::Active,
        default_group_id: None,
        created_at: now,
        updated_at: now,
    });
    projects_repo.seed(Project {
        id: proj_b_other_org,
        org_id: org_b,
        name: "other-org-proj".into(),
        slug: "other".into(),
        status: ProjectStatus::Active,
        default_group_id: None,
        created_at: now,
        updated_at: now,
    });

    // Channel + group：proj_a 有默认 group + 一个 healthy channel 指向上游
    let group_id = ChannelGroupId::new();
    let channel_id = ChannelId::new();
    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    ch_repo.seed_channel(ChannelRecord {
        channel_id,
        code: "wm".into(),
        name: "wm".into(),
        provider_type: "openai".into(),
        base_url: format!("{}/v1", upstream_uri),
        supported_models: vec![],
        status: "active".into(),
        health: "healthy".into(),
        timeout_ms: 60_000,
        max_retries: 1,
        created_at: now,
        updated_at: now,
    });
    ch_repo.seed_binding(group_id, channel_id, 10, 1);
    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "g".into(),
        strategy: "priority".into(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });
    grp_repo.seed_default(proj_a, group_id);

    let provider_router = ProviderRouter::new(ch_repo.clone(), grp_repo.clone());

    // Users via loader
    let loader = Arc::new(InMemoryLoader::new());

    let user_dev = UserId::new();
    let user_outsider = UserId::new();
    let user_super = UserId::new();

    {
        let mut orgs = HashMap::new();
        orgs.insert(org_a, OrgRole::Member);
        let mut projs = HashMap::new();
        projs.insert((org_a, proj_a), ProjectRole::Developer);
        loader.add_user(
            user_dev,
            UserRecord {
                orgs,
                projects: projs,
                platform: None,
            },
        );
    }
    loader.add_user(
        user_outsider,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
            platform: None,
        },
    );
    loader.add_user(
        user_super,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
            platform: Some(PlatformRole::SuperAdmin),
        },
    );

    // 组装 Repos —— users/orgs/memberships/api_keys 留默认空 InMemory，projects/channels 用我们 seed 过的
    let orgs_repo = Arc::new(gate_storage::InMemoryOrgRepo::new());
    orgs_repo.seed(Organization {
        id: org_a,
        name: "Acme".into(),
        slug: "acme".into(),
        owner_user_id: user_dev,
        status: OrgStatus::Active,
        billing_email: None,
        created_at: now,
        updated_at: now,
    });

    let repos = Repos {
        users: Arc::new(gate_storage::InMemoryUserRepo::new()),
        orgs: orgs_repo,
        projects: projects_repo,
        memberships: Arc::new(gate_storage::InMemoryMembershipRepo::new()),
        api_keys: Arc::new(gate_storage::InMemoryApiKeyRepo::new()),
        channels: ch_repo,
        channel_groups: grp_repo,
        identity_providers: Arc::new(gate_storage::InMemoryIdentityProviderRepo::new()),
        user_identities: Arc::new(gate_storage::InMemoryUserIdentityRepo::new()),
        oidc_states: Arc::new(gate_storage::InMemoryOidcStateRepo::new()),
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

    // fallback provider 也指向同一上游 —— 模拟无 channel 时仍可用
    let fallback = OpenAiProvider::new(format!("{}/v1", upstream_uri), "test-key").unwrap();
    let state = AppState::new(jwt, loader, repos)
        .with_provider(fallback)
        .with_provider_router(provider_router);

    let jwt_arc = state.jwt.clone();
    let router = build_router(state);

    let issue = |user: UserId, org: Option<OrgId>, is_super: bool| {
        jwt_arc
            .issue_access(
                *user.as_uuid(),
                Uuid::now_v7(),
                org.map(|o| *o.as_uuid()),
                is_super,
            )
            .unwrap()
            .0
    };

    Fixture {
        router,
        tok_dev: issue(user_dev, Some(org_a), false),
        tok_outsider: issue(user_outsider, None, false),
        tok_super: issue(user_super, None, true),
        proj_a,
        proj_b_other_org,
        org_a,
    }
}

async fn post_chat(
    router: &axum::Router,
    token: &str,
    project_header: Option<&str>,
) -> (StatusCode, Value) {
    let mut b = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json");
    if let Some(p) = project_header {
        b = b.header("x-kooix-project", p);
    }
    let req = b
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

#[tokio::test]
async fn ok_when_developer_provides_valid_project_header() {
    let f = fixture().await;
    let (status, body) =
        post_chat(&f.router, &f.tok_dev, Some(&f.proj_a.as_uuid().to_string())).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(body["choices"][0]["message"]["content"], "ok");
}

#[tokio::test]
async fn ok_when_no_project_header_falls_back() {
    // user_dev 没传 X-Kooix-Project → 走全局 fallback provider
    let f = fixture().await;
    let (status, _) = post_chat(&f.router, &f.tok_dev, None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn bad_request_when_project_header_not_uuid() {
    let f = fixture().await;
    let (status, body) = post_chat(&f.router, &f.tok_dev, Some("not-a-uuid")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "bad_request");
}

#[tokio::test]
async fn forbidden_when_project_belongs_to_other_org() {
    // user_dev 当前激活 Org A，传 proj_b_other_org（属于 Org B）→ 必须 403
    let f = fixture().await;
    let (status, body) = post_chat(
        &f.router,
        &f.tok_dev,
        Some(&f.proj_b_other_org.as_uuid().to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn forbidden_when_user_has_no_role_in_project() {
    // user_outsider 不属于 Org A，传 proj_a → 403
    // 注意：outsider JWT 里 org=None，先放进 Org A 上下文都不行 —— 因为 ctx.current_org 是 None
    let f = fixture().await;
    let (status, body) = post_chat(
        &f.router,
        &f.tok_outsider,
        Some(&f.proj_a.as_uuid().to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn super_admin_short_circuits_project_check() {
    // SuperAdmin 即使没显式角色也能用任何 project
    let f = fixture().await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", f.tok_super))
        .header("content-type", "application/json")
        .header("x-kooix-org", f.org_a.as_uuid().to_string())
        .header("x-kooix-project", f.proj_a.as_uuid().to_string())
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
