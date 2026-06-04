//! /v1/chat/completions 计费 E2E
//!
//! 用 wiremock 模拟 OpenAI 上游 + InMemoryOutboxRepo + InMemoryPricingRepo，
//! 验证：
//! 1. 非流式 chat → outbox 里有 1 条 UsageEvent（cost_micros 正确）
//! 2. 流式 chat → drain 完成后 outbox 里有 1 条 UsageEvent（用最后一帧 usage 算费）
//! 3. 找不到匹配 pricing rules → 不阻断，没事件入 outbox（warn-only）
//! 4. User 主体（非 ApiKey）→ 不计费（D4 阶段策略）

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Duration as ChronoDuration, Utc};
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_billing::{InMemoryOutboxRepo, InMemoryPricingRepo, OutboxRepo, PricingRepo, PricingRule};
use gate_core::id::{ApiKeyId, ChannelGroupId, ChannelId, OrgId, ProjectId, UserId};
use gate_providers::openai::OpenAiProvider;
use gate_server::loader::{ApiKeyRecord, InMemoryLoader, UserRecord};
use gate_server::state::Repos;
use gate_server::{AppState, build_router};
use http_body_util::BodyExt;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PLAINTEXT_KEY: &str = "sk-kg-test-billing-key-1234567890";

fn pricing_rule(model: &str, dimension: &str, unit: &str, rate: f64) -> PricingRule {
    PricingRule {
        id: Uuid::now_v7(),
        channel_id: None,
        model: model.to_string(),
        dimension: dimension.to_string(),
        unit: unit.to_string(),
        rate,
        conditions: json!({}),
        effective_from: chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap(),
        effective_until: None,
        priority: 0,
        description: None,
    }
}

struct Harness {
    router: axum::Router,
    outbox: Arc<InMemoryOutboxRepo>,
    user_jwt: String,
    /// 计费用：API key 主体路径用这个 key 调
    api_key_plain: &'static str,
}

struct DataPlaneHarness {
    router: axum::Router,
    outbox: Arc<InMemoryOutboxRepo>,
    api_key_plain: &'static str,
    api_key_id: ApiKeyId,
    project_id: ProjectId,
    org_id: OrgId,
    channel_id: ChannelId,
    channel_code: &'static str,
    channel_name: &'static str,
    model: &'static str,
}

type EmbeddingHarness = DataPlaneHarness;
type ImageHarness = DataPlaneHarness;
type AudioHarness = DataPlaneHarness;

async fn setup_with_billing(upstream: &MockServer, with_pricing: bool) -> Harness {
    setup_with_pricing(upstream, |pricing| {
        if with_pricing {
            pricing.seed_global("gpt-4o-mini", 0.15, 0.60);
        }
    })
    .await
}

async fn setup_with_pricing(
    upstream: &MockServer,
    seed_pricing: impl FnOnce(&Arc<InMemoryPricingRepo>),
) -> Harness {
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

    // Loader: 一个 user + 一个 api key
    let loader = Arc::new(InMemoryLoader::new());
    let user = UserId::new();
    loader.add_user(
        user,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
            platform: None,
        },
    );
    let api_key_id = ApiKeyId::new();
    let project_id = ProjectId::new();
    let org_id = OrgId::new();
    loader.add_api_key(
        PLAINTEXT_KEY,
        ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let provider = OpenAiProvider::new(format!("{}/v1", upstream.uri()), "test-key").unwrap();

    let pricing = Arc::new(InMemoryPricingRepo::new());
    seed_pricing(&pricing);
    let outbox = Arc::new(InMemoryOutboxRepo::new());

    let state = AppState::new(jwt, loader, Repos::in_memory())
        .with_provider(provider)
        .with_outbox(outbox.clone() as Arc<dyn OutboxRepo>)
        .with_pricing(pricing.clone() as Arc<dyn PricingRepo>);
    let jwt_issuer = state.jwt.clone();
    let router = build_router(state);
    let (tok, _) = jwt_issuer
        .issue_access(*user.as_uuid(), Uuid::now_v7(), None, false)
        .unwrap();

    Harness {
        router,
        outbox,
        user_jwt: tok,
        api_key_plain: PLAINTEXT_KEY,
    }
}

async fn setup_embeddings_with_pricing(
    upstream: &MockServer,
    seed_pricing: impl FnOnce(&Arc<InMemoryPricingRepo>),
) -> EmbeddingHarness {
    use gate_providers::ProviderRouter;
    use gate_storage::{
        ChannelGroupRecord, ChannelRecord, InMemoryChannelGroupRepo, InMemoryChannelRepo,
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

    let loader = Arc::new(InMemoryLoader::new());
    let api_key_id = ApiKeyId::new();
    let project_id = ProjectId::new();
    let org_id = OrgId::new();
    loader.add_api_key(
        PLAINTEXT_KEY,
        ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let group_id = ChannelGroupId::new();
    let channel_id = ChannelId::new();
    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let now = Utc::now();
    unsafe {
        std::env::set_var("KOOIX_CH_EMBED_WM_KEY", "test-key");
    }
    ch_repo.seed_channel(ChannelRecord {
        channel_id,
        code: "embed-wm".into(),
        name: "embed-wiremock".into(),
        provider_type: "plugin".into(),
        base_url: format!("{}/v1", upstream.uri()),
        supported_models: vec!["text-embedding-3-small".into()],
        status: "active".into(),
        health: "healthy".into(),
        timeout_ms: 60_000,
        max_retries: 1,
        rpm_limit: None,
        tpm_limit: None,
        tags: vec![],
        model_mapping: serde_json::json!({"plugin":{"version":1,"capabilities":{"chat":true,"streaming":true,"embeddings":true,"image":true,"audio":true},"auth":{"strategy":"none"},"preset":{"provider":"openai"}}}),
        balance: None,
        balance_updated_at: None,
        last_error: None,
        last_error_at: None,
        created_at: now,
        updated_at: now,
    });
    ch_repo.seed_binding(group_id, channel_id, 10, 1);
    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "embedding-default".into(),
        description: String::new(),
        strategy: "priority".into(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });
    grp_repo.seed_default(project_id, group_id);

    let pricing = Arc::new(InMemoryPricingRepo::new());
    seed_pricing(&pricing);
    let outbox = Arc::new(InMemoryOutboxRepo::new());
    let provider_router = ProviderRouter::new(ch_repo.clone(), grp_repo.clone());

    let mut repos = Repos::in_memory();
    repos.channels = ch_repo;
    repos.channel_groups = grp_repo;

    let state = AppState::new(jwt, loader, repos)
        .with_provider_router(provider_router)
        .with_outbox(outbox.clone() as Arc<dyn OutboxRepo>)
        .with_pricing(pricing.clone() as Arc<dyn PricingRepo>);
    let router = build_router(state);

    EmbeddingHarness {
        router,
        outbox,
        api_key_plain: PLAINTEXT_KEY,
        api_key_id,
        project_id,
        org_id,
        channel_id,
        channel_code: "embed-wm",
        channel_name: "embed-wiremock",
        model: "text-embedding-3-small",
    }
}

async fn setup_images_with_pricing(
    upstream: &MockServer,
    seed_pricing: impl FnOnce(&Arc<InMemoryPricingRepo>),
) -> ImageHarness {
    use gate_providers::ProviderRouter;
    use gate_storage::{
        ChannelGroupRecord, ChannelRecord, InMemoryChannelGroupRepo, InMemoryChannelRepo,
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

    let loader = Arc::new(InMemoryLoader::new());
    let api_key_id = ApiKeyId::new();
    let project_id = ProjectId::new();
    let org_id = OrgId::new();
    loader.add_api_key(
        PLAINTEXT_KEY,
        ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let group_id = ChannelGroupId::new();
    let channel_id = ChannelId::new();
    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let now = Utc::now();
    unsafe {
        std::env::set_var("KOOIX_CH_IMAGE_WM_KEY", "test-key");
    }
    ch_repo.seed_channel(ChannelRecord {
        channel_id,
        code: "image-wm".into(),
        name: "image-wiremock".into(),
        provider_type: "plugin".into(),
        base_url: format!("{}/v1", upstream.uri()),
        supported_models: vec!["dall-e-3".into()],
        status: "active".into(),
        health: "healthy".into(),
        timeout_ms: 60_000,
        max_retries: 1,
        rpm_limit: None,
        tpm_limit: None,
        tags: vec![],
        model_mapping: serde_json::json!({"plugin":{"version":1,"capabilities":{"chat":true,"streaming":true,"embeddings":true,"image":true,"audio":true},"auth":{"strategy":"none"},"preset":{"provider":"openai"}}}),
        balance: None,
        balance_updated_at: None,
        last_error: None,
        last_error_at: None,
        created_at: now,
        updated_at: now,
    });
    ch_repo.seed_binding(group_id, channel_id, 10, 1);
    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "image-default".into(),
        description: String::new(),
        strategy: "priority".into(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });
    grp_repo.seed_default(project_id, group_id);

    let pricing = Arc::new(InMemoryPricingRepo::new());
    seed_pricing(&pricing);
    let outbox = Arc::new(InMemoryOutboxRepo::new());
    let provider_router = ProviderRouter::new(ch_repo.clone(), grp_repo.clone());

    let mut repos = Repos::in_memory();
    repos.channels = ch_repo;
    repos.channel_groups = grp_repo;

    let state = AppState::new(jwt, loader, repos)
        .with_provider_router(provider_router)
        .with_outbox(outbox.clone() as Arc<dyn OutboxRepo>)
        .with_pricing(pricing.clone() as Arc<dyn PricingRepo>);
    let router = build_router(state);

    ImageHarness {
        router,
        outbox,
        api_key_plain: PLAINTEXT_KEY,
        api_key_id,
        project_id,
        org_id,
        channel_id,
        channel_code: "image-wm",
        channel_name: "image-wiremock",
        model: "dall-e-3",
    }
}

async fn setup_audio_with_pricing(
    upstream: &MockServer,
    model: &'static str,
    channel_code: &'static str,
    channel_name: &'static str,
    seed_pricing: impl FnOnce(&Arc<InMemoryPricingRepo>),
) -> AudioHarness {
    use gate_providers::ProviderRouter;
    use gate_storage::{
        ChannelGroupRecord, ChannelRecord, InMemoryChannelGroupRepo, InMemoryChannelRepo,
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

    let loader = Arc::new(InMemoryLoader::new());
    let api_key_id = ApiKeyId::new();
    let project_id = ProjectId::new();
    let org_id = OrgId::new();
    loader.add_api_key(
        PLAINTEXT_KEY,
        ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let group_id = ChannelGroupId::new();
    let channel_id = ChannelId::new();
    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let now = Utc::now();
    unsafe {
        std::env::set_var(
            format!(
                "KOOIX_CH_{}_KEY",
                channel_code
                    .to_uppercase()
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                    .collect::<String>()
            ),
            "test-key",
        );
    }
    ch_repo.seed_channel(ChannelRecord {
        channel_id,
        code: channel_code.into(),
        name: channel_name.into(),
        provider_type: "plugin".into(),
        base_url: format!("{}/v1", upstream.uri()),
        supported_models: vec![model.into()],
        status: "active".into(),
        health: "healthy".into(),
        timeout_ms: 60_000,
        max_retries: 1,
        rpm_limit: None,
        tpm_limit: None,
        tags: vec![],
        model_mapping: serde_json::json!({"plugin":{"version":1,"capabilities":{"chat":true,"streaming":true,"embeddings":true,"image":true,"audio":true},"auth":{"strategy":"none"},"preset":{"provider":"openai"}}}),
        balance: None,
        balance_updated_at: None,
        last_error: None,
        last_error_at: None,
        created_at: now,
        updated_at: now,
    });
    ch_repo.seed_binding(group_id, channel_id, 10, 1);
    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "audio-default".into(),
        description: String::new(),
        strategy: "priority".into(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });
    grp_repo.seed_default(project_id, group_id);

    let pricing = Arc::new(InMemoryPricingRepo::new());
    seed_pricing(&pricing);
    let outbox = Arc::new(InMemoryOutboxRepo::new());
    let provider_router = ProviderRouter::new(ch_repo.clone(), grp_repo.clone());

    let mut repos = Repos::in_memory();
    repos.channels = ch_repo;
    repos.channel_groups = grp_repo;

    let state = AppState::new(jwt, loader, repos)
        .with_provider_router(provider_router)
        .with_outbox(outbox.clone() as Arc<dyn OutboxRepo>)
        .with_pricing(pricing.clone() as Arc<dyn PricingRepo>);
    let router = build_router(state);

    AudioHarness {
        router,
        outbox,
        api_key_plain: PLAINTEXT_KEY,
        api_key_id,
        project_id,
        org_id,
        channel_id,
        channel_code,
        channel_name,
        model,
    }
}

async fn start_pg() -> (
    testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
    sqlx::PgPool,
) {
    use testcontainers::ImageExt;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    let tag = std::env::var("KOOIX_TEST_PG_TAG").unwrap_or_else(|_| "17-alpine".into());
    let container = Postgres::default().with_tag(&tag).start().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let pool = gate_storage::connect(&url, 4).await.unwrap();
    gate_storage::run_migrations(&pool).await.unwrap();
    (container, pool)
}

async fn seed_pg_usage_fixture(pool: &sqlx::PgPool, h: &DataPlaneHarness) {
    let user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, email, display_name, password_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(format!("{}-billing@test.dev", h.channel_code))
    .bind(format!("{} Billing Test", h.channel_name))
    .bind("$argon2id$v=19$m=19456,t=2,p=1$placeholder$placeholder")
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO organizations (id, name, slug, owner_user_id) VALUES ($1, $2, $3, $4)",
    )
    .bind(h.org_id.as_uuid())
    .bind(format!("{}-billing-org", h.channel_code))
    .bind(format!("{}-billing-org", h.channel_code))
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query("INSERT INTO projects (id, org_id, name, slug) VALUES ($1, $2, $3, $4)")
        .bind(h.project_id.as_uuid())
        .bind(h.org_id.as_uuid())
        .bind(format!("{}-billing-project", h.channel_code))
        .bind(format!("{}-billing-project", h.channel_code))
        .execute(pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO api_keys (id, project_id, name, key_hash, key_prefix, key_last4, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(h.api_key_id.as_uuid())
    .bind(h.project_id.as_uuid())
    .bind(format!("{}-billing-key", h.channel_code))
    .bind(format!(
        "fakehash_for_{}_billing_test_000000000000",
        h.channel_code
    ))
    .bind("sk-kg-test")
    .bind("test")
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO channels \
         (id, code, name, provider_type, base_url, config_enc, supported_models, status, health) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'active', 'healthy')",
    )
    .bind(h.channel_id.as_uuid())
    .bind(h.channel_code)
    .bind(h.channel_name)
    .bind("openai")
    .bind("http://example.invalid/v1")
    .bind(Vec::<u8>::new())
    .bind(vec![h.model.to_string()])
    .execute(pool)
    .await
    .unwrap();
}

async fn assert_request_log_projection(
    pool: &sqlx::PgPool,
    request_id: Uuid,
    h: &DataPlaneHarness,
    model: &str,
    tokens_in: i32,
    tokens_out: i32,
) {
    use gate_storage::{PgRequestLogRepo, RequestLogRepo};

    let projected_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_log_events WHERE request_id = $1")
            .bind(request_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        projected_count, 1,
        "request_log_events partitioned projection must receive committed request"
    );

    let partition_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pg_inherits WHERE inhparent = 'request_log_events'::regclass",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(
        partition_count >= 1,
        "request_log_events should have current/future monthly partitions"
    );

    let repo = PgRequestLogRepo::new(pool.clone());
    let partitions = repo.ensure_partitions(1).await.unwrap();
    assert!(
        !partitions.is_empty(),
        "request log partition helper should report ensured partitions"
    );
    assert_eq!(
        repo.prune_partitions(120, true).await.unwrap(),
        0,
        "fresh partitions should not be dropped in dry-run retention"
    );
    assert_eq!(
        repo.prune_details(3650).await.unwrap(),
        0,
        "fresh details retention should not delete rows"
    );

    let record = repo.find_by_request_id(request_id).await.unwrap();
    assert_eq!(record.model_actual, model);
    assert_eq!(record.tokens_in, tokens_in);
    assert_eq!(record.tokens_out, tokens_out);
    assert_eq!(record.channel_id, Some(*h.channel_id.as_uuid()));

    let page = repo
        .list(
            &gate_storage::RequestFilter {
                org_id: Some(*h.org_id.as_uuid()),
                search: Some(request_id.to_string()),
                ..Default::default()
            },
            None,
            10,
        )
        .await
        .unwrap();
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].request_id, request_id);
}

/// 等 spawn 出去的 emit_usage task 跑完。简单 sleep 几次让 tokio 调度。
async fn yield_for_emit() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn non_stream_apikey_emits_one_usage_event() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-billing-1",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "world" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1000, "completion_tokens": 500, "total_tokens": 1500 }
        })))
        .mount(&upstream)
        .await;

    let h = setup_with_billing(&upstream, true).await;

    let expected_request_id = Uuid::now_v7();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", h.api_key_plain))
        .header("content-type", "application/json")
        .header("x-request-id", expected_request_id.to_string())
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = h.router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = resp.into_body().collect().await.unwrap();

    yield_for_emit().await;

    let events = h.outbox.snapshot();
    assert_eq!(events.len(), 1, "expected exactly 1 outbox event");
    let ev = &events[0];
    assert_eq!(ev.request_id, expected_request_id);
    assert_eq!(ev.idempotency_key, Some(expected_request_id.to_string()));
    assert_eq!(ev.prompt_tokens, 1000);
    assert_eq!(ev.completion_tokens, 500);
    // 1000 * 0.15 / 1M + 500 * 0.60 / 1M = 0.00015 + 0.00030 = 0.00045 USD = 450 micros
    assert_eq!(ev.cost_micros, 450);
    assert_eq!(ev.status, 200);
    assert_eq!(ev.model, "gpt-4o-mini");
}

#[tokio::test]
#[ignore = "ADR-0004: image routing through plugin preset not yet wired for billing e2e"]
async fn images_apikey_emits_usage_event() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "created": 1710000000,
            "data": [
                { "url": "https://cdn.example.test/one.png", "revised_prompt": "one" },
                { "url": "https://cdn.example.test/two.png", "revised_prompt": "two" }
            ]
        })))
        .mount(&upstream)
        .await;

    let h = setup_images_with_pricing(&upstream, |pricing| {
        let mut rule = pricing_rule("dall-e-3", "per_image", "per_image", 0.08);
        rule.conditions = json!({"quality":"hd","size":"1024x1024"});
        pricing.seed(rule);
    })
    .await;

    let expected_request_id = Uuid::now_v7();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/images/generations")
        .header("authorization", format!("Bearer {}", h.api_key_plain))
        .header("content-type", "application/json")
        .header("x-request-id", expected_request_id.to_string())
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "dall-e-3",
                "prompt": "draw a gate",
                "n": 2,
                "size": "1024x1024",
                "quality": "hd"
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = h.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["data"].as_array().unwrap().len(), 2);

    yield_for_emit().await;

    let events = h.outbox.snapshot();
    assert_eq!(events.len(), 1, "expected exactly 1 image usage event");
    let ev = &events[0];
    assert_eq!(ev.request_id, expected_request_id);
    assert_eq!(ev.idempotency_key, Some(expected_request_id.to_string()));
    assert_eq!(ev.model, "dall-e-3");
    assert_eq!(ev.prompt_tokens, 0);
    assert_eq!(ev.completion_tokens, 0);
    assert_eq!(ev.image_units, 2);
    assert_eq!(ev.cost_micros, 160000);
    assert_eq!(ev.status, 200);
    assert_eq!(ev.channel_id, Some(*h.channel_id.as_uuid()));
    assert_eq!(
        ev.raw_usage.as_ref().unwrap()["endpoint"],
        "images.generations"
    );

    let (_pg, pool) = start_pg().await;
    seed_pg_usage_fixture(&pool, &h).await;
    gate_billing::consumer::commit_usage(&pool, ev)
        .await
        .unwrap();

    let usage_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM usage_records WHERE request_id = $1")
            .bind(expected_request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(usage_count, 1, "image event must commit usage_records");

    let request_event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_events WHERE request_id = $1")
            .bind(expected_request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        request_event_count, 1,
        "image event must commit request_events"
    );

    assert_request_log_projection(&pool, expected_request_id, &h, "dall-e-3", 0, 0).await;
}

#[tokio::test]
#[ignore = "ADR-0004: audio routing through plugin preset not yet wired for billing e2e"]
async fn audio_speech_apikey_emits_usage_event() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/speech"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "audio/mpeg")
                .set_body_bytes(vec![0_u8, 1, 2, 3, 4]),
        )
        .mount(&upstream)
        .await;

    let h = setup_audio_with_pricing(
        &upstream,
        "tts-1",
        "audio-speech-wm",
        "audio-speech-wiremock",
        |pricing| {
            pricing.seed(pricing_rule(
                "tts-1",
                "per_character_tts",
                "per_character",
                0.00001,
            ));
        },
    )
    .await;

    let expected_request_id = Uuid::now_v7();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/audio/speech")
        .header("authorization", format!("Bearer {}", h.api_key_plain))
        .header("content-type", "application/json")
        .header("x-request-id", expected_request_id.to_string())
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "tts-1",
                "input": "hello audio",
                "voice": "alloy",
                "response_format": "mp3"
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = h.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 5);

    yield_for_emit().await;

    let events = h.outbox.snapshot();
    assert_eq!(events.len(), 1, "expected exactly 1 audio speech event");
    let ev = &events[0];
    assert_eq!(ev.request_id, expected_request_id);
    assert_eq!(ev.idempotency_key, Some(expected_request_id.to_string()));
    assert_eq!(ev.model, "tts-1");
    assert_eq!(ev.prompt_tokens, 0);
    assert_eq!(ev.completion_tokens, 0);
    assert_eq!(ev.cost_micros, 110);
    assert_eq!(ev.status, 200);
    assert_eq!(ev.channel_id, Some(*h.channel_id.as_uuid()));
    let raw = ev.raw_usage.as_ref().unwrap();
    assert_eq!(raw["endpoint"], "audio.speech");
    assert_eq!(raw["tts_characters"], 11);
    assert_eq!(raw["response_bytes"], 5);

    let (_pg, pool) = start_pg().await;
    seed_pg_usage_fixture(&pool, &h).await;
    gate_billing::consumer::commit_usage(&pool, ev)
        .await
        .unwrap();

    let usage_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM usage_records WHERE request_id = $1")
            .bind(expected_request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        usage_count, 1,
        "audio speech event must commit usage_records"
    );

    let request_event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_events WHERE request_id = $1")
            .bind(expected_request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        request_event_count, 1,
        "audio speech event must commit request_events"
    );

    assert_request_log_projection(&pool, expected_request_id, &h, "tts-1", 0, 0).await;
}

#[tokio::test]
#[ignore = "ADR-0004: audio routing through plugin preset not yet wired for billing e2e"]
async fn audio_transcription_apikey_emits_usage_event() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "text": "hello from audio"
        })))
        .mount(&upstream)
        .await;

    let h = setup_audio_with_pricing(
        &upstream,
        "whisper-1",
        "audio-transcription-wm",
        "audio-transcription-wiremock",
        |pricing| {
            pricing.seed(pricing_rule(
                "whisper-1",
                "per_request",
                "per_request",
                0.006,
            ));
        },
    )
    .await;

    let expected_request_id = Uuid::now_v7();
    let boundary = "----kooix-audio-test-boundary";
    let body = format!(
        "--{boundary}\r\n\
Content-Disposition: form-data; name=\"model\"\r\n\r\n\
whisper-1\r\n\
--{boundary}\r\n\
Content-Disposition: form-data; name=\"language\"\r\n\r\n\
en\r\n\
--{boundary}\r\n\
Content-Disposition: form-data; name=\"file\"; filename=\"sample.wav\"\r\n\
Content-Type: audio/wav\r\n\r\n\
abc123\r\n\
--{boundary}--\r\n"
    );
    let req = Request::builder()
        .method("POST")
        .uri("/v1/audio/transcriptions")
        .header("authorization", format!("Bearer {}", h.api_key_plain))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header("x-request-id", expected_request_id.to_string())
        .body(Body::from(body))
        .unwrap();
    let resp = h.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["text"], "hello from audio");

    yield_for_emit().await;

    let events = h.outbox.snapshot();
    assert_eq!(
        events.len(),
        1,
        "expected exactly 1 audio transcription event"
    );
    let ev = &events[0];
    assert_eq!(ev.request_id, expected_request_id);
    assert_eq!(ev.idempotency_key, Some(expected_request_id.to_string()));
    assert_eq!(ev.model, "whisper-1");
    assert_eq!(ev.prompt_tokens, 0);
    assert_eq!(ev.completion_tokens, 0);
    assert_eq!(ev.cost_micros, 6000);
    assert_eq!(ev.status, 200);
    assert_eq!(ev.channel_id, Some(*h.channel_id.as_uuid()));
    let raw = ev.raw_usage.as_ref().unwrap();
    assert_eq!(raw["endpoint"], "audio.transcriptions");
    assert_eq!(raw["audio_bytes"], 6);
    assert_eq!(raw["language"], "en");
    assert_eq!(raw["filename"], "sample.wav");

    let (_pg, pool) = start_pg().await;
    seed_pg_usage_fixture(&pool, &h).await;
    gate_billing::consumer::commit_usage(&pool, ev)
        .await
        .unwrap();

    let usage_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM usage_records WHERE request_id = $1")
            .bind(expected_request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        usage_count, 1,
        "audio transcription event must commit usage_records"
    );

    let request_event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_events WHERE request_id = $1")
            .bind(expected_request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        request_event_count, 1,
        "audio transcription event must commit request_events"
    );

    assert_request_log_projection(&pool, expected_request_id, &h, "whisper-1", 0, 0).await;
}

#[tokio::test]
async fn embeddings_apikey_emits_usage_event() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{
                "object": "embedding",
                "index": 0,
                "embedding": [0.1, 0.2, 0.3]
            }],
            "model": "text-embedding-3-small",
            "usage": { "prompt_tokens": 1000, "total_tokens": 1000 }
        })))
        .mount(&upstream)
        .await;

    let h = setup_embeddings_with_pricing(&upstream, |pricing| {
        pricing.seed(pricing_rule(
            "text-embedding-3-small",
            "input_tokens",
            "per_million_tokens",
            0.02,
        ));
    })
    .await;

    let expected_request_id = Uuid::now_v7();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embeddings")
        .header("authorization", format!("Bearer {}", h.api_key_plain))
        .header("content-type", "application/json")
        .header("x-request-id", expected_request_id.to_string())
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "text-embedding-3-small",
                "input": "hello embeddings"
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = h.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["object"], "list");

    yield_for_emit().await;

    let events = h.outbox.snapshot();
    assert_eq!(events.len(), 1, "expected exactly 1 embedding usage event");
    let ev = &events[0];
    assert_eq!(ev.request_id, expected_request_id);
    assert_eq!(ev.idempotency_key, Some(expected_request_id.to_string()));
    assert_eq!(ev.model, "text-embedding-3-small");
    assert_eq!(ev.prompt_tokens, 1000);
    assert_eq!(ev.completion_tokens, 0);
    assert_eq!(ev.cost_micros, 20);
    assert_eq!(ev.status, 200);
    assert_eq!(ev.channel_id, Some(*h.channel_id.as_uuid()));
    assert_eq!(ev.raw_usage.as_ref().unwrap()["endpoint"], "embeddings");

    let (_pg, pool) = start_pg().await;
    seed_pg_usage_fixture(&pool, &h).await;
    gate_billing::consumer::commit_usage(&pool, ev)
        .await
        .unwrap();

    let usage_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM usage_records WHERE request_id = $1")
            .bind(expected_request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(usage_count, 1, "embedding event must commit usage_records");

    let request_event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_events WHERE request_id = $1")
            .bind(expected_request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        request_event_count, 1,
        "embedding event must commit request_events"
    );

    assert_request_log_projection(
        &pool,
        expected_request_id,
        &h,
        "text-embedding-3-small",
        1000,
        0,
    )
    .await;
}

#[tokio::test]
async fn non_stream_usage_event_keeps_raw_and_multimodal_cost_dimensions() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-mm-1",
            "model": "private-mm",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "world" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 100,
                "total_tokens": 1100,
                "cached_tokens": 400,
                "reasoning_tokens": 50,
                "image_units": 2,
                "audio_seconds": 120,
                "raw": { "vendor": "private", "meter": "m1" }
            }
        })))
        .mount(&upstream)
        .await;

    let h = setup_with_pricing(&upstream, |pricing| {
        pricing.seed(pricing_rule(
            "private-mm",
            "input_tokens",
            "per_million_tokens",
            1.0,
        ));
        pricing.seed(pricing_rule(
            "private-mm",
            "output_tokens",
            "per_million_tokens",
            2.0,
        ));
        pricing.seed(pricing_rule(
            "private-mm",
            "cached_input_tokens",
            "per_million_tokens",
            0.25,
        ));
        pricing.seed(pricing_rule(
            "private-mm",
            "reasoning_tokens",
            "per_million_tokens",
            4.0,
        ));
        pricing.seed(pricing_rule("private-mm", "per_image", "per_image", 0.08));
        pricing.seed(pricing_rule(
            "private-mm",
            "per_minute_audio",
            "per_minute",
            0.01,
        ));
    })
    .await;

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", h.api_key_plain))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "private-mm",
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = h.router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = resp.into_body().collect().await.unwrap();

    yield_for_emit().await;

    let events = h.outbox.snapshot();
    assert_eq!(events.len(), 1);
    let ev = &events[0];
    assert_eq!(ev.reasoning_tokens, 50);
    assert_eq!(ev.image_units, 2);
    assert_eq!(ev.audio_seconds, 120.0);
    assert_eq!(ev.raw_usage.as_ref().unwrap()["vendor"], "private");
    // uncached input 600u @ $1/M = 600µ, cached 400u @ $0.25/M = 100µ,
    // output 100u @ $2/M = 200µ, reasoning 50u @ $4/M = 200µ,
    // images 2 * $0.08 = 160000µ, audio 2min * $0.01 = 20000µ.
    assert_eq!(ev.cost_micros, 181100);
}

#[tokio::test]
async fn stream_apikey_emits_one_usage_event_from_final_frame() {
    let upstream = MockServer::start().await;
    // 最后一帧带 usage（OpenAI include_usage=true 的行为）
    let sse = concat!(
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"he\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"llo\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":500,\"total_tokens\":1500}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&upstream)
        .await;

    let h = setup_with_billing(&upstream, true).await;

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", h.api_key_plain))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Hi"}],
                "stream": true
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = h.router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    // Drain 整个 body —— 这会把流跑完，trigger emit
    let _bytes = resp.into_body().collect().await.unwrap().to_bytes();

    yield_for_emit().await;

    let events = h.outbox.snapshot();
    assert_eq!(
        events.len(),
        1,
        "expected exactly 1 outbox event from stream"
    );
    let ev = &events[0];
    assert_eq!(ev.prompt_tokens, 1000);
    assert_eq!(ev.completion_tokens, 500);
    assert_eq!(ev.cost_micros, 450);
    assert_eq!(ev.model, "gpt-4o-mini");
}

#[tokio::test]
async fn stream_without_usage_frame_emits_estimated_usage_event() {
    let upstream = MockServer::start().await;
    let sse = concat!(
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&upstream)
        .await;

    let h = setup_with_billing(&upstream, true).await;

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", h.api_key_plain))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Hello world!"}],
                "max_tokens": 100,
                "stream": true
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = h.router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _bytes = resp.into_body().collect().await.unwrap().to_bytes();

    yield_for_emit().await;

    let events = h.outbox.snapshot();
    assert_eq!(
        events.len(),
        1,
        "stream without usage must not silently skip billing"
    );
    let ev = &events[0];
    assert_eq!(ev.prompt_tokens, 3);
    assert_eq!(ev.completion_tokens, 100);
    assert_eq!(ev.raw_usage.as_ref().unwrap()["estimated"], true);
    // 3 input tokens @ $0.15/M = 0.45µ -> rounded 0; 100 output @ $0.60/M = 60µ.
    assert_eq!(ev.cost_micros, 60);
}

#[tokio::test]
async fn empty_pricing_rules_mean_no_billing_but_request_succeeds() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-nopricing",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        })))
        .mount(&upstream)
        .await;

    // with_pricing=false → 挂空 pricing repo，模拟没有匹配 rules。
    let h = setup_with_billing(&upstream, false).await;

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", h.api_key_plain))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = h.router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "request must still succeed");

    yield_for_emit().await;

    // pricing rules 缺失 → 没有 outbox 事件（warn-only）
    let events = h.outbox.snapshot();
    assert!(
        events.is_empty(),
        "expected no outbox events when pricing is missing, got {events:?}"
    );
}

#[tokio::test]
async fn no_pricing_for_unknown_model_skips_billing() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-unknown-model",
            "model": "exotic-model-9000",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        })))
        .mount(&upstream)
        .await;

    // with_pricing=true 但只 seed gpt-4o-mini，调 exotic-model-9000 → pricing miss
    let h = setup_with_billing(&upstream, true).await;

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", h.api_key_plain))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "exotic-model-9000",
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = h.router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    yield_for_emit().await;
    let events = h.outbox.snapshot();
    assert!(
        events.is_empty(),
        "expected no outbox events when model has no pricing"
    );
}

#[tokio::test]
async fn user_subject_chat_is_not_billed() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-user-subject",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        })))
        .mount(&upstream)
        .await;

    let h = setup_with_billing(&upstream, true).await;

    // 用 JWT user 主体调 —— D4 阶段策略：不计费
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", h.user_jwt))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = h.router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    yield_for_emit().await;
    let events = h.outbox.snapshot();
    assert!(
        events.is_empty(),
        "user subject must not produce billing events, got {events:?}"
    );
}

#[tokio::test]
async fn stream_request_injects_include_usage_into_upstream() {
    // 验证 D4 任务 1：handler 走流式时 upstream 收到的 body 必须有 stream_options.include_usage=true
    use wiremock::matchers::body_partial_json;
    let upstream = MockServer::start().await;
    let sse = concat!(
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(json!({
            "stream": true,
            "stream_options": { "include_usage": true }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .expect(1)
        .mount(&upstream)
        .await;

    let h = setup_with_billing(&upstream, true).await;

    // 客户端没传 stream_options，但 OpenAiProvider 必须替我们注入
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", h.api_key_plain))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Hi"}],
                "stream": true
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = h.router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = resp.into_body().collect().await.unwrap();
    // wiremock 的 .expect(1) 会在 drop 时检查 hit 次数
}

/// E1: ProviderRouter 选中 channel 后，channel_id 必须沿调用链传到 UsageEvent。
/// fallback 路径已被现有测试覆盖（channel_id=None）。
#[tokio::test]
async fn routed_chat_records_channel_id_in_outbox() {
    use gate_core::id::{ChannelGroupId, ChannelId};
    use gate_providers::ProviderRouter;
    use gate_storage::{
        ChannelGroupRecord, ChannelRecord, InMemoryChannelGroupRepo, InMemoryChannelRepo,
    };

    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-routed-1",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "routed!" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1000, "completion_tokens": 500, "total_tokens": 1500 }
        })))
        .mount(&upstream)
        .await;

    // ID setup
    let api_key_id = ApiKeyId::new();
    let project_id = ProjectId::new();
    let org_id = OrgId::new();
    let group_id = ChannelGroupId::new();
    let channel_id = ChannelId::new();

    // Channel + group
    // SAFETY: test is single-threaded at this point, no concurrent env reads
    unsafe {
        std::env::set_var("KOOIX_CH_WM_KEY", "test-key");
    }
    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let now = chrono::Utc::now();
    ch_repo.seed_channel(ChannelRecord {
        channel_id,
        code: "wm".into(),
        name: "wm-channel".into(),
        provider_type: "plugin".into(),
        base_url: format!("{}/v1", upstream.uri()),
        supported_models: vec![],
        status: "active".into(),
        health: "healthy".into(),
        timeout_ms: 60_000,
        max_retries: 1,
        rpm_limit: None,
        tpm_limit: None,
        tags: vec![],
        model_mapping: serde_json::json!({"plugin":{"version":1,"capabilities":{"chat":true,"streaming":true,"embeddings":true,"image":true,"audio":true},"auth":{"strategy":"none"},"preset":{"provider":"openai"}}}),
        balance: None,
        balance_updated_at: None,
        last_error: None,
        last_error_at: None,
        created_at: now,
        updated_at: now,
    });
    ch_repo.seed_binding(group_id, channel_id, 10, 1);
    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "g".into(),
        description: String::new(),
        strategy: "priority".into(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });
    grp_repo.seed_default(project_id, group_id);

    let provider_router = ProviderRouter::new(ch_repo.clone(), grp_repo.clone());

    // JWT
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

    // Loader: API key 绑定到上面的 project_id / org_id
    let plaintext = "sk-kg-test-routed-channel-key-aaaaa";
    let loader = Arc::new(InMemoryLoader::new());
    loader.add_api_key(
        plaintext,
        ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    // Pricing + outbox
    let pricing = Arc::new(InMemoryPricingRepo::new());
    pricing.seed_global("gpt-4o-mini", 0.15, 0.60);
    let outbox = Arc::new(InMemoryOutboxRepo::new());

    // 用真 ChannelRepo / Group repo（非 default in_memory）
    let mut repos = Repos::in_memory();
    repos.channels = ch_repo;
    repos.channel_groups = grp_repo;

    let state = AppState::new(jwt, loader, repos)
        .with_provider_router(provider_router)
        .with_outbox(outbox.clone() as Arc<dyn OutboxRepo>)
        .with_pricing(pricing.clone() as Arc<dyn PricingRepo>);
    // 不挂 fallback provider，强制走 router 路径
    let router = build_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {plaintext}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    let status = resp.status();
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8_lossy(&body_bytes);
    assert_eq!(status, StatusCode::OK, "response body: {body_str}");

    yield_for_emit().await;

    let events = outbox.snapshot();
    assert_eq!(events.len(), 1, "expected exactly 1 outbox event");
    let ev = &events[0];
    assert_eq!(
        ev.channel_id,
        Some(*channel_id.as_uuid()),
        "channel_id must propagate from ProviderRouter to UsageEvent"
    );
    assert_eq!(ev.prompt_tokens, 1000);
    assert_eq!(ev.completion_tokens, 500);
}
