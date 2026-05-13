//! OpenAI provider 集成测试 — 用 wiremock 模拟上游。

use futures::StreamExt;
use gate_providers::openai::OpenAiProvider;
use gate_providers::{ChatMessage, ChatRequest, Provider, ProviderError, Role};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_req(model: &str) -> ChatRequest {
    ChatRequest {
        model: model.into(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: "Hi".into(),
            name: None,
        }],
        temperature: Some(0.5),
        top_p: None,
        max_tokens: Some(16),
        stream: false,
        extra: Default::default(),
    }
}

#[tokio::test]
async fn non_stream_happy_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-abc",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "hello!" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7 }
        })))
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(format!("{}/v1", server.uri()), "test-key").unwrap();
    let resp = p.chat(make_req("gpt-4o-mini")).await.unwrap();
    assert_eq!(resp.id, "chatcmpl-abc");
    assert_eq!(resp.choices[0].message.content, "hello!");
    assert_eq!(resp.usage.total_tokens, 7);
}

#[tokio::test]
async fn stream_sse_parses_chunks_and_done() {
    let server = MockServer::start().await;

    let sse = concat!(
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
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
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(format!("{}/v1", server.uri()), "test-key").unwrap();
    let mut stream = p.chat_stream(make_req("gpt-4o-mini")).await.unwrap();

    let mut assembled = String::new();
    let mut last_finish = None;
    let mut count = 0;
    while let Some(item) = stream.next().await {
        let chunk = item.unwrap();
        count += 1;
        if let Some(content) = &chunk.choices[0].delta.content {
            assembled.push_str(content);
        }
        if let Some(reason) = chunk.choices[0].finish_reason {
            last_finish = Some(reason);
        }
    }
    assert_eq!(count, 4);
    assert_eq!(assembled, "hello");
    assert_eq!(last_finish, Some(gate_providers::FinishReason::Stop));
}

#[tokio::test]
async fn upstream_401_maps_to_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": { "message": "invalid api key" }
        })))
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(format!("{}/v1", server.uri()), "bad-key").unwrap();
    let err = p.chat(make_req("gpt-4o-mini")).await.unwrap_err();
    assert!(matches!(err, ProviderError::Auth(_)), "got {err:?}");
}

#[tokio::test]
async fn upstream_429_carries_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "7")
                .set_body_json(serde_json::json!({"error":{"message":"slow down"}})),
        )
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(format!("{}/v1", server.uri()), "k").unwrap();
    let err = p.chat(make_req("gpt-4o-mini")).await.unwrap_err();
    match err {
        ProviderError::RateLimited { retry_after_ms } => {
            assert_eq!(retry_after_ms, Some(7_000));
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}
