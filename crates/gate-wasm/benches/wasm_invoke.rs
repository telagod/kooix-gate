//! 0.4.37 wasm transform bench — 测量 wasmtime invoke_hook 单次开销。
//!
//! Run: cargo bench --package gate-wasm --bench wasm_invoke
//!
//! 三个对比点：
//! - identity passthrough (no module)：0 cost baseline
//! - real wasm hook (memory copy 模块)：单次 transform 调用开销
//! - real wasm hook with payload 1KB / 10KB / 100KB：scaling
//!
//! 0.5.0+ 把 gate-providers 集成层 + 真实 e2e 加进 plugin_vs_builtin 矩阵。

use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use gate_wasm::{HookContext, HookKind, WasmHost, WasmHostConfig, WasmtimeHost};
use std::sync::Arc;
use tokio::runtime::Runtime;

const COPY_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "gate_alloc") (param i32) (result i32) i32.const 4096)
  (func (export "chat_request_transform")
    (param $ptr i32) (param $len i32) (result i64)
    (local $i i32)
    (block $done
      (loop $copy
        (br_if $done (i32.ge_s (local.get $i) (local.get $len)))
        (i32.store8
          (i32.add (i32.const 4096) (local.get $i))
          (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $copy)))
    (i64.or
      (i64.shl (i64.extend_i32_u (i32.const 4096)) (i64.const 32))
      (i64.extend_i32_u (local.get $len))))
)
"#;

fn build_host_with_module(payload_size: usize) -> (Arc<dyn WasmHost>, Bytes) {
    let rt = Runtime::new().unwrap();
    let host: Arc<dyn WasmHost> =
        Arc::new(WasmtimeHost::new(WasmHostConfig::default()).unwrap());
    let module_bytes = wat::parse_str(COPY_WAT).unwrap();
    let sha = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(&module_bytes);
        hex::encode(h.finalize())
    };
    rt.block_on(async {
        host.load_module("bench-ch", &module_bytes, &sha).await.unwrap();
    });
    let payload = Bytes::from(vec![b'x'; payload_size]);
    (host, payload)
}

fn bench_invoke_hook(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("wasm_invoke_hook");

    for size in [128usize, 1024, 10_240] {
        let (host, payload) = build_host_with_module(size);
        group.bench_with_input(BenchmarkId::new("memory_copy", size), &size, |b, _| {
            b.iter(|| {
                rt.block_on(async {
                    let result = host
                        .invoke_hook(
                            "bench-ch",
                            HookKind::ChatRequest,
                            black_box(payload.clone()),
                            HookContext::default(),
                        )
                        .await
                        .unwrap();
                    black_box(result)
                })
            });
        });
    }

    group.finish();
}

fn bench_no_module_passthrough(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let host: Arc<dyn WasmHost> =
        Arc::new(WasmtimeHost::new(WasmHostConfig::default()).unwrap());
    let payload = Bytes::from_static(b"baseline-no-module");

    c.bench_function("wasm_no_module_passthrough", |b| {
        b.iter(|| {
            rt.block_on(async {
                let result = host
                    .invoke_hook(
                        "no-module-channel",
                        HookKind::ChatRequest,
                        black_box(payload.clone()),
                        HookContext::default(),
                    )
                    .await
                    .unwrap();
                black_box(result)
            })
        });
    });
}

criterion_group!(benches, bench_invoke_hook, bench_no_module_passthrough);
criterion_main!(benches);
