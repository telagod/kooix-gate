//! Host-side Criterion benchmarks for `pii-redact-v1`.
//!
//! Three scenarios mirror the runtime hot paths declared in `README.md`：
//!
//! 1. `typical_4kb_payload` —— 真实 chat completion 体扩展到 ~4KB,
//!    走 `redact_json`，对标 release p99 < 1ms / p50 < 500us。
//! 2. `single_email_string` —— 极小裸文本，走 `redact_text`，
//!    对标 release p99 < 10us。
//! 3. `sse_chunk` —— `fixtures/sse_chunk.txt` 走 `redact_text`，
//!    流式 chunk 路径的回归 baseline。
//!
//! 运行：
//! ```bash
//! cargo bench --bench redact
//! cargo bench --bench redact -- --quick    # smoke
//! ```

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use pii_redact_v1::default_redactor;
use std::hint::black_box;

/// 把 ~340B 的 chat_request fixture 扩展到 ~4KB —— 复制 `messages[1].content`
/// 字符串若干次填进新的 user message，使整体 payload 接近 chat 业务最常见的
/// "system prompt + 几轮上下文" 体量。
fn build_typical_4kb_payload() -> String {
    let base = include_str!("../fixtures/chat_request.json");
    let mut value: serde_json::Value = serde_json::from_str(base).expect("fixture is valid json");

    // 取首条 user 内容作为扩张种子（含若干 PII 字段，更贴近真实命中分布）
    let seed = value["messages"][1]["content"]
        .as_str()
        .unwrap_or("alice@example.com 13800138000 sk-test-xxxxxxxxxxxxxxxxxxxx")
        .to_string();

    // 4 KB ≈ 4096 字节，扣掉外壳 ~300B，目标内容字段 ≈ 3800B。
    let target_bytes = 3800usize;
    let mut buf = String::with_capacity(target_bytes + seed.len());
    while buf.len() < target_bytes {
        buf.push_str(&seed);
        buf.push(' ');
    }

    if let Some(messages) = value["messages"].as_array_mut() {
        messages.push(serde_json::json!({
            "role": "user",
            "content": buf,
        }));
    }

    let out = serde_json::to_string(&value).expect("re-serialize");
    debug_assert!(out.len() >= 3500, "payload should be roughly 4KB, got {}", out.len());
    out
}

fn bench_typical_4kb_payload(c: &mut Criterion) {
    let body = build_typical_4kb_payload();
    let redactor = default_redactor();

    let mut group = c.benchmark_group("typical_4kb_payload");
    group.throughput(Throughput::Bytes(body.len() as u64));
    group.bench_function("redact_json", |b| {
        b.iter(|| {
            let (out, stats) = redactor.redact_json(black_box(&body), None);
            black_box((out, stats));
        });
    });
    group.finish();
}

fn bench_single_email_string(c: &mut Criterion) {
    let input = "contact alice@example.com please";
    let redactor = default_redactor();

    let mut group = c.benchmark_group("single_email_string");
    group.throughput(Throughput::Bytes(input.len() as u64));
    group.bench_function("redact_text", |b| {
        b.iter(|| {
            let (out, stats) = redactor.redact_text(black_box(input), None);
            black_box((out, stats));
        });
    });
    group.finish();
}

fn bench_sse_chunk(c: &mut Criterion) {
    let chunk = include_str!("../fixtures/sse_chunk.txt");
    let redactor = default_redactor();

    let mut group = c.benchmark_group("sse_chunk");
    group.throughput(Throughput::Bytes(chunk.len() as u64));
    group.bench_function("redact_text", |b| {
        b.iter(|| {
            let (out, stats) = redactor.redact_text(black_box(chunk), None);
            black_box((out, stats));
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_typical_4kb_payload,
    bench_single_email_string,
    bench_sse_chunk,
);
criterion_main!(benches);
