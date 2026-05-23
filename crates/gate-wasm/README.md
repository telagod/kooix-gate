# gate-wasm

ADR-0003 v0 WASM Plugin runtime for Kooix Gate — wasmtime 26 + async + 3 transform hooks + panic-safe fallback + Prometheus.

> 0.4.21-0.4.27 实装，0.4.41-0.4.60 集成到 `gate-providers` + `kgctl` + Helm + Grafana。

## 职责

承载 [ADR-0003 v0](../../docs/architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md) 三个 transform hook 的执行：

- `chat_request_transform(body, ctx) -> body`
- `chat_response_transform(body, ctx) -> body`
- `stream_chunk_transform(chunk, ctx) -> chunk?`

不掌控网络 / secret / 路由 / 计费——那些归 host。详见 [docs/wasm-plugin-abi.md](../../docs/wasm-plugin-abi.md)。

## 模块

| 文件 | 职责 |
|------|------|
| `src/host.rs` | `WasmHost` async trait + `HookKind` + `HookContext` |
| `src/wasmtime_host.rs` | `WasmtimeHost` 实装：`Config::async_support(true) + consume_fuel(true)`；ABI v0 i64 解码（`ptr<<32 | len`） |
| `src/fallback.rs` | `invoke_with_fallback`（`catch_unwind` + `AssertUnwindSafe`，panic 兜底） + Prometheus `gate_plugin_wasm_calls_total` |
| `src/limits.rs` | `ResourceLimits { max_memory_bytes: 16 MiB, max_cpu_ms: 50, max_modules_per_channel: 1 }` |
| `src/error.rs` | `WasmError` enum（Load / Instantiate / Call / Timeout / OutOfMemory / DigestMismatch / HostDenied / Panic） |

## 资源限制

- 默认：内存 16 MiB / CPU 50ms wall（fuel = `max_cpu_ms × 1_000_000_000`）
- 通过 channel manifest `security.wasm.{max_memory_bytes,max_cpu_ms}` 覆盖
- 超限走 `WasmError::OutOfMemory` / `Timeout`，metrics label `status="oom"|"timeout"`，host fallback 到 manifest 原始路径

## 失败语义

所有失败 fail-safe 落到 manifest 原始链路；用户请求**不会失败**：

| status | 说明 |
|--------|------|
| `ok` | 正常 transform |
| `timeout` | CPU 超 fuel |
| `oom` | linear memory 超 limit |
| `panic` | wasm trap，host catch_unwind 兜底 |
| `digest_mismatch` | manifest sha256 不匹配 |
| `no_module` | manifest 没声明此 hook |
| `host_denied` | host 政策拒绝（reserved） |
| `load_error` / `instantiate_error` / `call_error` | 编译 / 实例化 / 调用错误 |

## Bench

```bash
cargo bench --package gate-wasm --bench wasm_invoke
```

包含 `memory_copy/128`、`/1024`、`/10240` 三档 payload。

## 集成 SDK

- [Rust](../gate-wasm-sdk/) — 推荐生产用
- [AssemblyScript](../../sdks/gate-wasm-sdk-as/) — 前端工程师友好

## 参考

- [ADR-0003 v0](../../docs/architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md)
- [docs/wasm-plugin-abi.md](../../docs/wasm-plugin-abi.md) § 0.4.x 实装对账
- [docs/wasm-runbook.md](../../docs/wasm-runbook.md) — 故障处置
- [docs/product-gaps.md](../../docs/product-gaps.md) — v0.5.0 候选（auto-mount / host functions / wit-bindgen）
