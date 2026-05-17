//! Custom HTTP provider plugin integration tests.

use futures::StreamExt;
use gate_providers::{ChatMessage, ChatRequest, CustomHttpProvider, Provider, Role};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_req(stream: bool) -> ChatRequest {
    ChatRequest {
        model: "odd-model".into(),
        messages: vec![ChatMessage::text(Role::User, "hello private")],
        max_tokens: Some(32),
        stream,
        ..Default::default()
    }
}

#[tokio::test]
async fn plugin_maps_private_non_stream_request_and_response() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/private/chat"))
        .and(header("x-api-key", "secret-key"))
        .and(body_json(json!({
            "modelName": "odd-model",
            "prompt": "hello private",
            "stream": false,
            "limit": 32
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "rid": "resp-1",
            "model_name": "odd-model-native",
            "result": { "text": "mapped ok", "finish": "done" },
            "usage": { "input": 7, "output": 2 }
        })))
        .mount(&upstream)
        .await;

    let manifest = json!({
        "plugin": {
            "request": {
                "chat_path": "/private/chat",
                "headers": { "X-Api-Key": "{{api_key}}" },
                "body": {
                    "modelName": "{{model}}",
                    "prompt": "{{last_user_message}}",
                    "stream": "{{stream}}",
                    "limit": "{{max_tokens}}"
                }
            },
            "response": {
                "openai_compatible": false,
                "id_path": "rid",
                "model_path": "model_name",
                "content_path": "result.text",
                "finish_reason_path": "result.finish",
                "usage": {
                    "prompt_tokens_path": "usage.input",
                    "completion_tokens_path": "usage.output"
                }
            }
        }
    });

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "secret-key",
        manifest,
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let resp = provider.chat(make_req(false)).await.unwrap();
    assert_eq!(resp.id, "resp-1");
    assert_eq!(resp.model, "odd-model-native");
    assert_eq!(resp.choices[0].message.content_text(), "mapped ok");
    assert_eq!(
        resp.choices[0].finish_reason,
        Some(gate_providers::FinishReason::Stop)
    );
    assert_eq!(resp.usage.total_tokens, 9);
}

#[tokio::test]
async fn plugin_normalizes_private_sse_stream() {
    let upstream = MockServer::start().await;
    let sse = concat!(
        ": keepalive\r\n",
        "event: token\r\n",
        "data: {\"payload\":{\"rid\":\"s1\",\"model_name\":\"native\",\"speaker\":\"assistant\"}}\r\n\r\n",
        "data: {\"payload\":{\"token\":\"he\"}}\n\n",
        "data: {\"payload\":{\"token\":\"llo\"}}\n\n",
        "data: {\"payload\":{\"finish\":\"done\",\"usage\":{\"input\":4,\"output\":2}}}\n\n",
        "data: EOF\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/private/chat"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&upstream)
        .await;

    let manifest = json!({
        "plugin": {
            "request": { "chat_path": "/private/chat" },
            "stream": {
                "openai_compatible": false,
                "event_path": "payload",
                "id_path": "rid",
                "model_path": "model_name",
                "role_path": "speaker",
                "content_path": "token",
                "finish_reason_path": "finish",
                "done": ["EOF"],
                "usage": {
                    "prompt_tokens_path": "usage.input",
                    "completion_tokens_path": "usage.output"
                }
            }
        }
    });

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "secret-key",
        manifest,
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let mut stream = provider.chat_stream(make_req(true)).await.unwrap();
    let mut content = String::new();
    let mut finish = None;
    let mut usage = None;
    while let Some(item) = stream.next().await {
        let chunk = item.unwrap();
        if let Some(c) = &chunk.choices[0].delta.content {
            content.push_str(c);
        }
        if chunk.choices[0].finish_reason.is_some() {
            finish = chunk.choices[0].finish_reason;
        }
        if chunk.usage.is_some() {
            usage = chunk.usage;
        }
    }

    assert_eq!(content, "hello");
    assert_eq!(finish, Some(gate_providers::FinishReason::Stop));
    assert_eq!(usage.unwrap().total_tokens, 6);
}

#[tokio::test]
async fn preset_openai_compatible_posts_normalized_request_and_streams_usage() {
    let upstream = MockServer::start().await;
    let sse = concat!(
        "data: {\"id\":\"cmpl-1\",\"model\":\"gpt-compatible\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"cmpl-1\",\"model\":\"gpt-compatible\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"preset\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"cmpl-1\",\"model\":\"gpt-compatible\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1,\"total_tokens\":4}}\n\n",
        "data: [DONE]\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer preset-key"))
        .and(body_json(json!({
            "model": "odd-model",
            "messages": [{ "role": "user", "content": "hello private" }],
            "max_tokens": 32,
            "stream": true,
            "stream_options": { "include_usage": true }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "preset-key",
        json!({ "plugin": { "preset": { "provider": "openai_compatible" } } }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let mut stream = provider.chat_stream(make_req(true)).await.unwrap();
    let mut content = String::new();
    let mut usage = None;
    while let Some(item) = stream.next().await {
        let chunk = item.unwrap();
        if let Some(c) = &chunk.choices[0].delta.content {
            content.push_str(c);
        }
        if chunk.usage.is_some() {
            usage = chunk.usage;
        }
    }

    assert_eq!(content, "preset");
    assert_eq!(usage.unwrap().total_tokens, 4);
}

#[tokio::test]
async fn preset_anthropic_messages_posts_native_body_and_normalizes_response() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "anthropic-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .and(body_json(json!({
            "model": "odd-model",
            "max_tokens": 32,
            "messages": [{ "role": "user", "content": "hello private" }],
            "stream": false
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg-1",
            "model": "claude-native",
            "content": [{ "type": "text", "text": "anthropic mapped" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 5, "output_tokens": 2, "cache_read_input_tokens": 1 }
        })))
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "anthropic-key",
        json!({ "plugin": { "preset": { "provider": "anthropic_messages" } } }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let resp = provider.chat(make_req(false)).await.unwrap();
    assert_eq!(resp.id, "msg-1");
    assert_eq!(resp.model, "claude-native");
    assert_eq!(resp.choices[0].message.content_text(), "anthropic mapped");
    assert_eq!(
        resp.choices[0].finish_reason,
        Some(gate_providers::FinishReason::Stop)
    );
    assert_eq!(resp.usage.prompt_tokens, 5);
    assert_eq!(resp.usage.completion_tokens, 2);
    assert_eq!(resp.usage.total_tokens, 7);
    assert_eq!(resp.usage.cached_tokens, 1);
}
