//! Criterion micro-benchmarks for data-plane hot paths outside provider routing.
//!
//! These benches deliberately use in-memory repos and no external Redis/PG so they
//! stay cheap in local verification while still walking the real Axum quota
//! middleware and billing/outbox enqueue code paths.
//!
//! Run: cargo bench --package gate-server --bench hot_paths
//!
//! ## TODO (0.4.98 占位 → 0.5.x 实装)
//!
//! 真正的 chat e2e bench 需要 mock 上游 + criterion 量 chat handler 内部各 stage
//! 耗时（route → adapt → execute → settle）。目前 hot_paths 只覆盖 quota + billing
//! 微观路径，没量"从 request 进 axum 到 response 出 axum"端到端 latency。
//!
//! 0.5.x 实装方向：
//!
//! 1. 用 `wiremock` 起 mock OpenAI upstream（已有于 tests/）
//! 2. 用 reqwest 直接打 axum::Router 内部（绕过 TCP）
//! 3. criterion group "chat_e2e" + "chat_stream_e2e"，分别量非流与流
//! 4. 输出 baseline 写到 `bench/results/chat_e2e.json`，CI compare
//!
//! 与 [crates/gate-providers/benches/plugin_vs_builtin] 区别：那里量 provider
//! 适配层，本 bench 量 server 层（含 metrics / quota / billing 旁路）。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::from_fn_with_state;
use axum::response::IntoResponse;
use axum::routing::post;
use chrono::{Duration as ChronoDuration, Utc};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use futures::stream;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_billing::{InMemoryOutboxRepo, InMemoryPricingRepo, OutboxRepo, PricingRepo};
use gate_core::id::{ApiKeyId, OrgId, ProjectId};
use gate_providers::{
    ChatChoice, ChatMessage, ChatRequest, ChatResponse, FinishReason, Provider, ProviderResult,
    Role, Usage,
};
use gate_server::billing_emit::{BillingCtx, emit_usage};
use gate_server::loader::{ApiKeyRecord, InMemoryLoader};
use gate_server::middleware::quota_enforce;
use gate_server::state::{AppState, Repos};
use gate_storage::{InMemoryQuotaRepo, QuotaRecord};
use rust_decimal::Decimal;
use tower::ServiceExt;
use uuid::Uuid;

const API_KEY: &str = "sk-bench-quota-hot-path";

#[derive(Clone)]
struct StaticProvider;

#[async_trait::async_trait]
impl Provider for StaticProvider {
    fn name(&self) -> &'static str {
        "bench-static"
    }

    async fn chat(&self, req: ChatRequest) -> ProviderResult<ChatResponse> {
        Ok(ChatResponse {
            id: "chatcmpl-bench".to_string(),
            model: req.model,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage::text(Role::Assistant, "ok"),
                finish_reason: Some(FinishReason::Stop),
            }],
            usage: Usage {
                prompt_tokens: 8,
                completion_tokens: 4,
                total_tokens: 12,
                ..Default::default()
            },
            request_id: None,
            upstream_metadata: None,
        })
    }

    async fn chat_stream(
        &self,
        _req: ChatRequest,
    ) -> ProviderResult<
        futures::stream::BoxStream<'static, ProviderResult<gate_providers::ChatStreamChunk>>,
    > {
        Ok(Box::pin(stream::empty()))
    }
}

#[derive(Clone)]
struct QuotaBench {
    router: Router,
}

async fn ok_handler() -> impl IntoResponse {
    StatusCode::OK
}

fn test_jwt() -> JwtIssuer {
    JwtIssuer::new(
        b"bench-secret-32-bytes-minimum-ok!!",
        "kg-bench",
        "console",
        TokenLifetimes {
            access: ChronoDuration::minutes(15),
            refresh: ChronoDuration::days(1),
        },
    )
    .unwrap()
}

fn quota_record(
    scope_kind: &str,
    scope_id: Uuid,
    dimension: &str,
    limit_value: &str,
    model_filter: Option<&str>,
) -> QuotaRecord {
    let now = Utc::now();
    QuotaRecord {
        id: Uuid::now_v7(),
        scope_kind: scope_kind.to_string(),
        scope_id,
        dimension: dimension.to_string(),
        model_filter: model_filter.map(str::to_string),
        limit_value: limit_value.parse::<Decimal>().unwrap(),
        window_seconds: Some(60),
        mode: "enforce".to_string(),
        enabled: true,
        created_at: now,
        updated_at: now,
    }
}

fn build_quota_bench(quota_kind: QuotaKind) -> QuotaBench {
    let api_key_id = ApiKeyId::new();
    let project_id = ProjectId::new();
    let org_id = OrgId::new();

    let loader = Arc::new(InMemoryLoader::new());
    loader.add_api_key(
        API_KEY,
        ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let mut repos = Repos::in_memory();
    if let Some(record) = match quota_kind {
        QuotaKind::None => None,
        QuotaKind::RpmNoBody => Some(quota_record(
            "api_key",
            *api_key_id.as_uuid(),
            "rpm",
            "1000000",
            None,
        )),
        QuotaKind::BudgetWithBody => Some(quota_record(
            "api_key",
            *api_key_id.as_uuid(),
            "daily_budget_usd",
            "999999",
            Some("gpt-*"),
        )),
    } {
        let quotas = Arc::new(InMemoryQuotaRepo::new());
        quotas.seed(record);
        repos.quotas = quotas;
    }

    let state = AppState::new(test_jwt(), loader, repos).with_provider(StaticProvider);
    let router = Router::new()
        .route("/bench", post(ok_handler))
        .layer(from_fn_with_state(state.clone(), quota_enforce))
        .with_state(state);

    QuotaBench { router }
}

#[derive(Clone, Copy)]
enum QuotaKind {
    None,
    RpmNoBody,
    BudgetWithBody,
}

fn quota_request(body: &'static [u8]) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/bench")
        .header("authorization", format!("Bearer {API_KEY}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

fn chat_body() -> &'static [u8] {
    br#"{"model":"gpt-4o-mini","messages":[{"role":"user","content":"bench quota hot path"}],"max_tokens":64}"#
}

fn bench_quota_checks(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("quota_check_hot_path");

    for (name, kind, body) in [
        (
            "no_quota_valid_apikey",
            QuotaKind::None,
            b"{}" as &'static [u8],
        ),
        ("rpm_quota_no_body_parse", QuotaKind::RpmNoBody, b"{}"),
        (
            "budget_quota_body_parse_model_filter",
            QuotaKind::BudgetWithBody,
            chat_body(),
        ),
    ] {
        let bench = build_quota_bench(kind);
        group.bench_function(name, |b| {
            b.to_async(&rt).iter(|| async {
                let response = bench
                    .router
                    .clone()
                    .oneshot(quota_request(body))
                    .await
                    .unwrap();
                black_box(response.status());
            });
        });
    }

    group.finish();
}

fn usage() -> Usage {
    Usage {
        prompt_tokens: 128,
        completion_tokens: 64,
        total_tokens: 192,
        cached_tokens: 16,
        ..Default::default()
    }
}

fn billing_ctx() -> BillingCtx {
    let request_id = Uuid::now_v7();
    BillingCtx {
        api_key_id: Uuid::now_v7(),
        project_id: Uuid::now_v7(),
        org_id: Uuid::now_v7(),
        channel_id: Some(Uuid::now_v7()),
        group_id: Some(Uuid::now_v7()),
        model: "gpt-4o-mini".to_string(),
        request_id,
        idempotency_key: request_id.to_string(),
    }
}

fn bench_request_log_enqueue(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("request_log_enqueue_hot_path");

    let pricing = Arc::new(InMemoryPricingRepo::new());
    pricing.seed_global("gpt-4o-mini", 0.15, 0.60);
    let pricing: Arc<dyn PricingRepo> = pricing;
    let outbox: Arc<dyn OutboxRepo> = Arc::new(InMemoryOutboxRepo::new());

    group.bench_function("billing_emit_inmemory_outbox", |b| {
        b.to_async(&rt).iter(|| async {
            emit_usage(
                Some(outbox.clone()),
                Some(pricing.clone()),
                billing_ctx(),
                usage(),
                200,
            )
            .await;
        });
    });

    group.finish();
}

// 0.4.140（按 0.4.98 TODO step 1）：chat provider dispatch micro-bench。
// 量 StaticProvider.chat() 单调用开销作为 baseline，不接 axum router
// （axum dispatch + auth + quota middleware 单独由 bench_quota_checks 覆盖）。
// 0.4.141：扩到 stream + extra params 2 个 case，覆盖更典型流量画像。
fn bench_chat_provider_dispatch(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("chat_provider_dispatch");

    let provider: Arc<dyn Provider> = Arc::new(StaticProvider);

    group.bench_function("static_provider_chat_call", |b| {
        b.to_async(&rt).iter(|| {
            let p = provider.clone();
            async move {
                let req = ChatRequest {
                    model: "gpt-4o-mini".to_string(),
                    messages: vec![ChatMessage::text(Role::User, "bench")],
                    max_tokens: Some(64),
                    ..Default::default()
                };
                let resp = p.chat(req).await.unwrap();
                black_box(resp);
            }
        });
    });

    // 0.4.141: chat 带 extra params (top_p / temperature / response_format)
    group.bench_function("static_provider_chat_call_with_extra", |b| {
        b.to_async(&rt).iter(|| {
            let p = provider.clone();
            async move {
                let mut req = ChatRequest {
                    model: "gpt-4o-mini".to_string(),
                    messages: vec![ChatMessage::text(Role::User, "bench")],
                    max_tokens: Some(64),
                    temperature: Some(0.7),
                    top_p: Some(0.9),
                    ..Default::default()
                };
                req.extra
                    .insert("response_format".to_string(), serde_json::json!({"type": "json_object"}));
                req.extra
                    .insert("seed".to_string(), serde_json::json!(42));
                let resp = p.chat(req).await.unwrap();
                black_box(resp);
            }
        });
    });

    // 0.4.141: chat 多 message 历史（10 条对话）
    group.bench_function("static_provider_chat_call_10_messages", |b| {
        b.to_async(&rt).iter(|| {
            let p = provider.clone();
            async move {
                let mut msgs = Vec::with_capacity(10);
                for i in 0..10 {
                    let role = if i % 2 == 0 { Role::User } else { Role::Assistant };
                    msgs.push(ChatMessage::text(role, format!("msg-{i}")));
                }
                let req = ChatRequest {
                    model: "gpt-4o-mini".to_string(),
                    messages: msgs,
                    max_tokens: Some(64),
                    ..Default::default()
                };
                let resp = p.chat(req).await.unwrap();
                black_box(resp);
            }
        });
    });

    group.finish();
}

criterion_group!(
    hot_path_benches,
    bench_quota_checks,
    bench_request_log_enqueue,
    bench_chat_provider_dispatch
);
criterion_main!(hot_path_benches);
