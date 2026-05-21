//! ADR-0001 verification: plugin runtime vs compile-time provider 性能对比。
//!
//! 同一个 wiremock endpoint，分别用 `OpenAiProvider`（编译期 fast-path）和
//! `CustomHttpProvider` + `openai_compatible` preset（plugin runtime）发 chat 请求，
//! 对比单次 `chat()` 调用耗时。
//!
//! ADR-0001 verification 要求：plugin runtime ≤ 编译期 × 1.05（5% 性能预算）。
//!
//! Run: cargo bench --package gate-providers --bench plugin_vs_builtin
//!
//! 解读：跑完看 `criterion --baseline` 报告里 `plugin_runtime` 与 `builtin` 的 mean
//! ratio。HTTP / SSE / JSON 解析等开销两边都付，差异主要来自 manifest 解释器
//! （placeholder render / auth header build / endpoint url template 评估）。

use std::sync::Arc;
use std::time::Duration;

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use gate_providers::openai::OpenAiProvider;
use gate_providers::types::{ChatMessage, ChatRequest, MessageContent, Role};
use gate_providers::{CustomHttpProvider, Provider, ProviderOpts};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RESP_BODY: &str = r#"{
    "id": "chatcmpl-bench",
    "object": "chat.completion",
    "created": 1730000000,
    "model": "gpt-4o-mini",
    "choices": [{
        "index": 0,
        "message": {"role": "assistant", "content": "ok"},
        "finish_reason": "stop"
    }],
    "usage": {"prompt_tokens": 4, "completion_tokens": 1, "total_tokens": 5}
}"#;

fn sample_request() -> ChatRequest {
    ChatRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some(MessageContent::Text("ping".to_string())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        temperature: Some(0.0),
        max_tokens: Some(1),
        stream: false,
        ..Default::default()
    }
}

async fn setup_mock() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(RESP_BODY, "application/json"))
        .mount(&server)
        .await;
    server
}

fn opts() -> ProviderOpts {
    let mut opts = ProviderOpts::default();
    opts.timeout_ms = 5_000;
    opts
}

fn build_builtin(base_url: &str) -> Arc<OpenAiProvider> {
    Arc::new(
        OpenAiProvider::new_with_opts(base_url.to_string(), "sk-bench".to_string(), opts())
            .expect("build OpenAiProvider"),
    )
}

fn build_plugin(base_url: &str) -> Arc<CustomHttpProvider> {
    let manifest = json!({
        "plugin": {
            "preset": { "provider": "openai_compatible" }
        }
    });
    Arc::new(
        CustomHttpProvider::new_with_opts(
            base_url.to_string(),
            "sk-bench".to_string(),
            manifest,
            opts(),
        )
        .expect("build CustomHttpProvider"),
    )
}

fn bench_chat_compile_time(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(setup_mock());
    let base = server.uri();
    let provider = build_builtin(&base);
    let req = sample_request();

    let mut group = c.benchmark_group("chat_request");
    group.measurement_time(Duration::from_secs(8));
    group.sample_size(60);
    group.bench_function("builtin_openai", |b| {
        b.to_async(&rt).iter(|| {
            let provider = provider.clone();
            let req = req.clone();
            async move {
                black_box(provider.chat(req).await.unwrap());
            }
        });
    });
    group.finish();

    drop(server);
}

fn bench_chat_plugin_runtime(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(setup_mock());
    let base = server.uri();
    let provider = build_plugin(&base);
    let req = sample_request();

    let mut group = c.benchmark_group("chat_request");
    group.measurement_time(Duration::from_secs(8));
    group.sample_size(60);
    group.bench_function("plugin_openai_compatible", |b| {
        b.to_async(&rt).iter(|| {
            let provider = provider.clone();
            let req = req.clone();
            async move {
                black_box(provider.chat(req).await.unwrap());
            }
        });
    });
    group.finish();

    drop(server);
}

criterion_group!(
    plugin_vs_builtin,
    bench_chat_compile_time,
    bench_chat_plugin_runtime,
);
criterion_main!(plugin_vs_builtin);
