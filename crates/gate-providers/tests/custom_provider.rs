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
