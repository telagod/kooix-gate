//! /v1/chat/completions 计费 E2E
//!
//! 用 wiremock 模拟 OpenAI 上游 + InMemoryOutboxRepo + InMemoryPricingRepo，
//! 验证：
//! 1. 非流式 chat → outbox 里有 1 条 UsageEvent（cost_micros 正确）
//! 2. 流式 chat → drain 完成后 outbox 里有 1 条 UsageEvent（用最后一帧 usage 算费）
//! 3. 没挂 pricing → 不阻断，没事件入 outbox（warn-only）
//! 4. User 主体（非 ApiKey）→ 不计费（D4 阶段策略）

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_billing::{InMemoryOutboxRepo, InMemoryPricingRepo, OutboxRepo, PricingRepo};
use gate_core::id::{ApiKeyId, OrgId, ProjectId, UserId};
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

struct Harness {
    router: axum::Router,
    outbox: Arc<InMemoryOutboxRepo>,
    user_jwt: String,
    /// 计费用：API key 主体路径用这个 key 调
    api_key_plain: &'static str,
}

async fn setup_with_billing(upstream: &MockServer, with_pricing: bool) -> Harness {
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

    // Pricing: gpt-4o-mini $0.15/M in, $0.60/M out
    let pricing = Arc::new(InMemoryPricingRepo::new());
    if with_pricing {
        pricing.seed_global("gpt-4o-mini", 0.15, 0.60);
    }
    let outbox = Arc::new(InMemoryOutboxRepo::new());

    let mut state = AppState::new(jwt, loader, Repos::in_memory())
        .with_provider(provider)
        .with_outbox(outbox.clone() as Arc<dyn OutboxRepo>);
    if with_pricing {
        state = state.with_pricing(pricing.clone() as Arc<dyn PricingRepo>);
    }
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
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = resp.into_body().collect().await.unwrap();

    yield_for_emit().await;

    let events = h.outbox.snapshot();
    assert_eq!(events.len(), 1, "expected exactly 1 outbox event");
    let ev = &events[0];
    assert_eq!(ev.prompt_tokens, 1000);
    assert_eq!(ev.completion_tokens, 500);
    // 1000 * 0.15 / 1M + 500 * 0.60 / 1M = 0.00015 + 0.00030 = 0.00045 USD = 450 micros
    assert_eq!(ev.cost_micros, 450);
    assert_eq!(ev.status, 200);
    assert_eq!(ev.model, "gpt-4o-mini");
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
async fn no_pricing_means_no_billing_but_request_succeeds() {
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

    // with_pricing=false → 没挂 pricing
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

    // pricing 未挂 → 没有 outbox 事件（warn-only）
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
    unsafe { std::env::set_var("KOOIX_CH_WM_KEY", "test-key"); }
    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let now = chrono::Utc::now();
    ch_repo.seed_channel(ChannelRecord {
        channel_id,
        code: "wm".into(),
        name: "wm-channel".into(),
        provider_type: "openai".into(),
        base_url: format!("{}/v1", upstream.uri()),
        supported_models: vec![],
        status: "active".into(),
        health: "healthy".into(),
        timeout_ms: 60_000,
        max_retries: 1,
        rpm_limit: None,
            tpm_limit: None,
            tags: vec![],
            model_mapping: serde_json::Value::Object(Default::default()),
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
