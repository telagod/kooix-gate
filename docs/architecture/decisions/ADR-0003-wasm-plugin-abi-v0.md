# ADR-0003: WASM Plugin ABI v0（M3 收口 / 0.4.16 PoC）

- Status: **PoC accepted (0.4.16)** — host hook 设计 + sample manifest 落地，runtime 实现留 0.5.0
- Deciders: telagod
- Affected: `crates/gate-providers/src/wasm_plugin/`（待落地）, `docs/wasm-plugin-abi.md`, `docs/architecture/decisions/ADR-0001-providers-as-plugin.md`

## Context

ADR-0001 / ADR-0002 完成后，HTTP Plugin manifest v1 已经覆盖 23 个 preset，fast-path
runtime 把 4 个高频 provider 拉回编译期水准。但仍有两类需求 manifest 难以表达：

1. **复杂请求/响应变换**：deterministic prompt rewrite、token censor、
   field projection；manifest dot-path + template DSL 写不出。
2. **私有协议 SSE 整流**：vendor 的 SSE 帧编码偏离常见格式，需要自定义 parser
   而不是 manifest `stream.event_path`。

[`docs/wasm-plugin-abi.md`](../../wasm-plugin-abi.md) 已经有设计稿（2026-05-20），
0.4.x 这次收口的目标：把设计稿冻结为 ADR-0003 v0，并落地 sample manifest，
让 0.5.0 可以直接进 runtime 实现。

## Decision

### v0 ABI 边界

WASM 插件**仅**作为受限 transform runtime，不替代 manifest，不能掌控网络/secret/路由。

| Host 控制 | WASM 控制 |
|----------|----------|
| 网络出站（reqwest + sandbox） | request body transform |
| Secret 存取（envelope encryption） | response body transform |
| 路由 / quota / billing / audit | streaming chunk transform |
| 计时 / 资源限制 | 纯 deterministic computation |

### v0 host functions（最小集）

```text
// host -> wasm
plugin_init(manifest_json: string) -> void
plugin_chat_request_transform(body: bytes, ctx: ChatTransformContext) -> bytes
plugin_chat_response_transform(body: bytes, ctx: ChatTransformContext) -> bytes
plugin_stream_chunk_transform(chunk: bytes, ctx: StreamTransformContext) -> bytes | null

// wasm -> host
host_log(level: u8, msg: string)
host_get_secret_slot(slot_name: string) -> string  // host 提取 secret，不暴露 key material
host_record_metric(name: string, value: f64)
```

### v0 限制（hard limits）

- **CPU**：单次 transform ≤ 50ms wall clock，超时 host 杀死并降级到 manifest 路径
- **Memory**：linear memory ≤ 16 MiB，超量 OOM kill
- **No I/O**：禁 fs / net / clock 系统调用；只能通过 host function
- **Deterministic**：禁 random / time，host 注入 monotonic clock 用于 metric
- **Single instance per channel**：模块编译一次，按 channel 实例化，无跨 channel 共享 state

### Sample manifest（v0 引入字段）

```toml
# examples/manifest-registry/wasm-transform.toml（PoC sample）
[plugin]
version = 1

[plugin.metadata]
name = "deterministic-prompt-rewriter"
vendor = "kooix-gate-examples"
tags = ["wasm", "transform", "experimental"]

[plugin.capabilities]
chat = true
streaming = true

[plugin.security]
# 0.5.0+ 才接 runtime；现在这个字段仅占位
wasm_module = "modules/prompt_rewriter.wasm"
wasm_module_sha256 = "TBD"
wasm_max_memory_bytes = 16777216
wasm_max_cpu_ms = 50
wasm_hooks = ["chat_request_transform", "chat_response_transform"]
```

### 与 HTTP Plugin manifest 的关系

```text
请求 → channel 选 manifest →
  if manifest.security.wasm_module:
    → wasm.plugin_init() (lazy)
    → wasm.plugin_chat_request_transform(body, ctx)
    → host: HTTP request to base_url
    → wasm.plugin_chat_response_transform(body, ctx)
  else:
    → manifest 解释器 / fast-path
```

WASM 插件是 manifest 流程的 **inner transform layer**，不替代 manifest，
manifest 仍负责 auth / endpoint / SSE basic normalization。

### Fallback 与可观测

- WASM 模块 panic / OOM / timeout → host `tracing::error!` + 降级到 manifest 原始路径，进程不挂
- 每次 transform 上报 metric：`gate_plugin_wasm_calls_total{channel, hook, status}`
- audit log 记录 `wasm_module_sha256` 字段，方便复现

## Consequences

### Positive

- M3 完成最后一项：HTTP Plugin manifest v1 + WASM transform 双层扩展面定型
- 用户能写 deterministic Rust / Go / AssemblyScript 处理复杂 transform，不必等编译期 provider
- ADR-0001 战略主线（"渠道插件化优先不写 Rust"）的 escape hatch — manifest 写不出再上 WASM

### Negative / Risks

- **PoC 仅落 ABI 设计 + sample**：0.4.16 不出 runtime 实现，wasmtime crate 引入留 0.5.0
- **WASM cold start**：模块 instantiation 有~ms 级开销，fast-path 不应走 WASM
- **跨语言 ABI 维护成本**：Rust SDK 先做，AssemblyScript / Go 等社区跟进

### Verification

- [x] 0.4.16：ADR-0003 v0 设计冻结
- [x] 0.4.16：`examples/manifest-registry/wasm-transform.toml` sample 占位
- [x] 0.4.16：`docs/wasm-plugin-abi.md` 同步指向 ADR-0003
- [ ] 0.5.0：`crates/gate-providers/src/wasm_plugin/` runtime 落地（wasmtime 引擎 + secrets bridge + sandbox）
- [ ] 0.5.0：`SecurityManifest::wasm_*` 字段接 runtime（当前仅 schema 占位）
- [ ] 0.5.0：sample Rust SDK + golden test 模块

## References

- [ADR-0001 Providers as Plugin](./ADR-0001-providers-as-plugin.md)
- [ADR-0002 Fast-path Runtime](./ADR-0002-fastpath-runtime.md)
- [WASM Plugin ABI 设计稿](../../wasm-plugin-abi.md)
- [HTTP Plugin Manifest 文档](../../plugin-manifest.md)
