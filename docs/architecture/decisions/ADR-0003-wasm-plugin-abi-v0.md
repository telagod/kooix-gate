# ADR-0003: WASM Plugin ABI v0（M3 收口 / 0.4.16 → 0.4.60 实装）

- Status: **Superseded (by [ADR-0006](./ADR-0006-wasm-abi-v1-component-model.md); was Implemented 0.4.16 PoC → 0.4.60 完整产品形态)** — v0 ABI 作为 dual-run window 兼容基线保留，新功能走 ADR-0006 component-model v1。原 v0 全栈落地内容（host hook + sample manifest + wasmtime runtime + Rust SDK + AssemblyScript SDK + 3 hook 含 SSE + ProviderRouter 集成 + e2e + Prometheus + Grafana + runbook + signature schema，0.4.16-0.4.60）保留为 v1 迁移期对照。
- Deciders: telagod
- Affected: `crates/gate-wasm/`, `crates/gate-wasm-sdk/`, `sdks/gate-wasm-sdk-as/`, `crates/gate-providers/src/{custom_provider,plugin_manifest,router}/`, `crates/kgctl/src/{wasm,plugin}.rs`, `docs/wasm-plugin-abi.md`, `docs/wasm-runbook.md`, `docs/wasm-sdk-as.md`, `docs/manifest-registry-signature.md`

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

- **0.4.x 已落 v0 完整产品形态**：runtime + 3 hook + Rust/AS SDK + e2e + observability。**v0.5.0+ 风险**：ABI v0 手写 calling convention 维护成本高，wit-bindgen / component-model 是迁移方向（见 G-103）。
- **WASM cold start**：模块 instantiation 有 ~ms 级开销，fast-path 不应走 WASM；`Module::serialize` 持久化缓存留 0.5.0+（见 G-104）。
- **跨语言 ABI 维护成本**：Rust SDK 与 AssemblyScript SDK 已 v0 落地；Go / Python / Zig 等留待 v0.6.0+（见 G-301）。

### Verification

#### M3 0.4.16 PoC（设计冻结）

- [x] 0.4.16：ADR-0003 v0 设计冻结
- [x] 0.4.16：`examples/manifest-registry/wasm-transform.toml` sample 占位
- [x] 0.4.16：`docs/wasm-plugin-abi.md` 同步指向 ADR-0003

#### M3 0.4.21-0.4.60 完整产品形态实装

- [x] 0.4.21-0.4.27：`crates/gate-wasm` runtime（wasmtime 26 + async + cranelift + consume_fuel + ResourceLimits + 3 hook + fallback panic-safe + Prometheus）
- [x] 0.4.21：`crates/gate-wasm-sdk` Rust SDK（`gate_alloc` + `export_chat_request!/response!/stream_chunk!` macros）
- [x] 0.4.23：`SecurityManifest::wasm` 字段接通 runtime（`WasmModuleManifest { module, module_sha256, max_memory_bytes, max_cpu_ms, hooks }`）
- [x] 0.4.41-0.4.46：`CustomHttpProvider` 集成 `wasm_host` + `with_wasm_host` builder + chat / chat_stream 全链路 wiremock e2e
- [x] 0.4.45：`kgctl wasm verify|inspect`（sha256 + manifest 片段 + wasmparser 校验 export 表）
- [x] 0.4.51-0.4.52：SSE pipeline 内 `stream_chunk_transform` 真接通 + 4 个 wiremock e2e（含 SSE chunk）
- [x] 0.4.53-0.4.54：Manifest registry 签名 schema typed（`kind/value/key_id/alg`）+ 格式校验
- [x] 0.4.55-0.4.56：AssemblyScript SDK npm package（`@kooix-gate/wasm-sdk-as`）+ `examples/wasm-transform-as/`
- [x] 0.4.57：`ProviderRouter::with_wasm_host` / `wasm_host()` setter+getter
- [x] 0.4.58：Prometheus `metrics::describe_counter!("gate_plugin_wasm_calls_total", ...)` 注册
- [x] 0.4.31：Helm chart 暴露 wasm 资源限制 values
- [x] 0.4.34：Grafana dashboard 含 WASM panel
- [x] 0.4.x：`docs/wasm-runbook.md` 故障处置手册

#### v0.5.0 候选（设计已就绪，等启动会议筛选 — 详见 [docs/product-gaps.md](../../product-gaps.md)）

- [ ] G-001：cosign / sigstore-rs / minisign 真实公钥验签链（0.4.54 schema 已落，runtime 调用未起）
- [ ] G-002：WASM 模块外部存储 + auto-mount（0.4.57 setter/getter 已落，BlobStore + auto-load 未起）
- [ ] G-003：host functions 真实暴露（host_log / host_get_secret_slot / host_record_metric）
- [ ] G-004：stream event-by-event transform（当前 chunk 是 raw bytes 穿透，未按 SSE event 解码后再喂）
- [ ] G-101：AssemblyScript SDK npm publish
- [ ] G-102：管理面 wasm form UI
- [ ] G-103：ABI v1 走 wit-bindgen + component-model
- [ ] G-104：WASM 编译产物持久化缓存（`Module::serialize`）

## References

- [ADR-0001 Providers as Plugin](./ADR-0001-providers-as-plugin.md)
- [ADR-0002 Fast-path Runtime](./ADR-0002-fastpath-runtime.md)
- [WASM Plugin ABI 设计稿](../../wasm-plugin-abi.md)
- [HTTP Plugin Manifest 文档](../../plugin-manifest.md)
- [Product gaps v0.4.60 → v0.5.0](../../product-gaps.md)
- [WASM Runbook](../../wasm-runbook.md)
- [Manifest Registry Signature](../../manifest-registry-signature.md)
- [AssemblyScript SDK](../../wasm-sdk-as.md)
