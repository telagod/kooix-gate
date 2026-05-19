//! /v1/chat/completions E2E：用 wiremock 假装 OpenAI 上游。
//!
//! 验证：
//! - 鉴权：未携带 auth → 401
//! - 非流式：handler → provider → wiremock → handler 返回完整 JSON
//! - 流式：handler 转发 SSE chunks，客户端拿到累计的 content
//! - 上游 401 → 502 + normalized authentication_error code

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::id::UserId;
use gate_providers::openai::OpenAiProvider;
use gate_server::loader::{InMemoryLoader, UserRecord};
use gate_server::state::Repos;
use gate_server::{AppState, build_router};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn setup(upstream: &MockServer) -> (axum::Router, String) {
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
    let user = UserId::new();
    loader.add_user(
        user,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
            platform: None,
        },
    );

    let provider = OpenAiProvider::new(format!("{}/v1", upstream.uri()), "test-key").unwrap();
    let state = AppState::new(jwt, loader, Repos::in_memory()).with_provider(provider);
    let jwt = state.jwt.clone();
    let router = build_router(state);

    let (tok, _) = jwt
        .issue_access(*user.as_uuid(), Uuid::now_v7(), None, false)
        .unwrap();
    (router, tok)
}

#[tokio::test]
async fn chat_completions_requires_auth() {
    let upstream = MockServer::start().await;
    let (router, _tok) = setup(&upstream).await;

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
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
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn chat_completions_non_stream_passthrough() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-xyz",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "world" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 3, "completion_tokens": 1, "total_tokens": 4 }
        })))
        .mount(&upstream)
        .await;

    let (router, tok) = setup(&upstream).await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {tok}"))
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
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["id"], "chatcmpl-xyz");
    assert_eq!(body["choices"][0]["message"]["content"], "world");
    assert_eq!(body["usage"]["total_tokens"], 4);
}

#[tokio::test]
async fn chat_completions_stream_passthrough() {
    let upstream = MockServer::start().await;
    let sse = concat!(
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"he\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"llo\"},\"finish_reason\":null}]}\n\n",
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

    let (router, tok) = setup(&upstream).await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {tok}"))
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
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("text/event-stream"), "ct={ct}");

    // 收集所有 body
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();

    // 应有 3 个 data: 行（最后一个 [DONE] 由 provider 层吃掉）
    let data_lines: Vec<&str> = text.lines().filter(|l| l.starts_with("data: ")).collect();
    assert_eq!(data_lines.len(), 3, "got lines={data_lines:?}");

    // 累计 content == "hello"
    let mut content = String::new();
    for line in &data_lines {
        let json_part = line.strip_prefix("data: ").unwrap();
        let v: Value = serde_json::from_str(json_part).unwrap();
        if let Some(c) = v["choices"][0]["delta"]["content"].as_str() {
            content.push_str(c);
        }
    }
    assert_eq!(content, "hello");
}

#[tokio::test]
async fn responses_non_stream_thin_adapter_to_chat() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "resp-chat-xyz",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "world" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 3, "completion_tokens": 1, "total_tokens": 4 }
        })))
        .mount(&upstream)
        .await;

    let (router, tok) = setup(&upstream).await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("authorization", format!("Bearer {tok}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "instructions": "Be terse",
                "input": "Hi",
                "max_output_tokens": 32
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["id"], "resp-chat-xyz");
    assert_eq!(body["object"], "response");
    assert_eq!(body["status"], "completed");
    assert_eq!(body["output_text"], "world");
    assert_eq!(body["output"][0]["type"], "message");
    assert_eq!(body["output"][0]["content"][0]["type"], "output_text");
    assert_eq!(body["usage"]["total_tokens"], 4);
}

#[tokio::test]
async fn responses_stream_thin_adapter_to_chat_sse() {
    let upstream = MockServer::start().await;
    let sse = concat!(
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"he\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"llo\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}\n\n",
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

    let (router, tok) = setup(&upstream).await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("authorization", format!("Bearer {tok}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "input": "Hi",
                "stream": true
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("text/event-stream"), "ct={ct}");

    let text = String::from_utf8(
        resp.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(text.contains("\"type\":\"response.output_text.delta\""));
    assert!(text.contains("\"delta\":\"he\""));
    assert!(text.contains("\"delta\":\"llo\""));
    assert!(text.contains("\"type\":\"response.completed\""));
}

#[tokio::test]
async fn chat_completions_upstream_auth_failure_maps_to_502() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({"error": {"message": "bad key"}})),
        )
        .mount(&upstream)
        .await;

    let (router, tok) = setup(&upstream).await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {tok}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "x"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "authentication_error");
    assert_eq!(body["error"]["type"], "authentication_error");
    assert_eq!(body["error"]["message"], "upstream auth failed");
    assert_eq!(body["error"]["upstream_status"], 401);
}

#[tokio::test]
async fn chat_completions_upstream_404_maps_to_model_not_found() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({"error": {"message": "model gone"}})),
        )
        .mount(&upstream)
        .await;

    let (router, tok) = setup(&upstream).await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {tok}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "missing-model",
                "messages": [{"role": "user", "content": "x"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "model_not_found");
    assert_eq!(body["error"]["type"], "invalid_request_error");
    assert_eq!(body["error"]["upstream_status"], 404);
}

#[tokio::test]
async fn chat_completions_400_when_no_provider_configured() {
    let upstream = MockServer::start().await;
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
    let user = UserId::new();
    loader.add_user(
        user,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
            platform: None,
        },
    );
    // 不挂 provider
    let state = AppState::new(jwt, loader, Repos::in_memory());
    let jwt = state.jwt.clone();
    let router = build_router(state);
    let (tok, _) = jwt
        .issue_access(*user.as_uuid(), Uuid::now_v7(), None, false)
        .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {tok}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "x"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let _ = upstream; // unused on this path
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
