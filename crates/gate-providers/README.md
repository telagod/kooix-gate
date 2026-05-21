# gate-providers

> Kooix Gate 上游 LLM 适配层。把 OpenAI / Anthropic / Azure / Bedrock / 私有协议
> 等异构上游统一抽象为 `Provider` / `EmbeddingProvider` / `ImageProvider` / `AudioProvider` trait，
> 由 `ProviderRouter` 按 channel group 策略选路。

## 模块边界

```
src/
├── lib.rs                      # trait + pub use re-export
├── adapt.rs                    # OpenAI-compat 互转 helper
├── error.rs                    # ProviderError + NormalizedProviderErrorKind
├── retry.rs                    # RetryConfig + 错误归一
├── sse.rs                      # 通用 SSE decoder
├── types.rs                    # ChatRequest/Response/Stream*Chunk/Usage 等
├── capabilities.rs             # ProviderCapability matrix
├── plugin_preset.rs            # 18+ 内置 preset (vllm/lm_studio/anthropic_messages/...)
│
├── openai.rs                   # 编译期 fast-path
├── anthropic.rs                # 编译期 fast-path
├── azure.rs                    # 编译期 fast-path
├── bedrock.rs                  # 编译期 fast-path（含 SigV4 通过 plugin runtime）
├── cohere.rs                   # ⚠ 0.2.1 deprecated，0.3.0 退役（plugin preset 替代）
├── deepseek.rs                 # ⚠ 0.2.1 deprecated
├── gemini.rs                   # ⚠ 0.2.1 deprecated
├── mistral.rs                  # ⚠ 0.2.1 deprecated
├── ollama.rs                   # ⚠ 0.2.1 deprecated
│
├── router/                     # ProviderRouter — channel group 路由
│   ├── mod.rs                  # ProviderRouter struct + 主 impl
│   ├── trace.rs                # RouteCandidateTrace/SkipTrace/MissReason/DecisionTrace/RuntimeSnapshot
│   ├── routed.rs               # RoutedProvider/EmbeddingProvider/ImageProvider/AudioProvider
│   ├── metrics.rs              # ChannelMetrics + InMemoryChannelRateLimiter + InflightTracker
│   ├── selection.rs            # priority/weighted_random/round_robin/least_conn/least_latency
│   ├── helpers.rs              # fallback_models/resolve_model_mapping/capability/env secret
│   └── builder.rs              # build_provider*/build_embedding*/build_image*/build_audio*
│
├── custom_provider/            # HTTP Plugin runtime
│   ├── mod.rs                  # CustomHttpProvider + Provider/EmbeddingProvider impl
│   ├── sandbox.rs              # PluginHttpSandbox — SSRF guard + DNS rebind + metadata host
│   ├── replay.rs               # StreamMapper + replay_plugin_sse + normalize_plugin_sse
│   ├── sigv4.rs                # AWS SigV4 + HMAC-SHA256 primitives
│   └── secrets.rs              # secret slot normalization + env fallback
│
└── plugin_manifest/            # Plugin manifest v1 schema
    ├── mod.rs                  # PluginManifest 顶层 + AuthManifest/RequestManifest/...
    ├── validate.rs             # validate_auth/response/stream/probe/template/secret_slot 全套校验
    └── upgrade.rs              # v0 → v1 自动升级路径
```

## 公共 API

```rust
use gate_providers::{
    Provider, EmbeddingProvider, ImageProvider, AudioProvider,  // trait
    ProviderRouter, RoutedProvider, ChannelMetrics,             // router
    CustomHttpProvider, replay_plugin_sse,                       // plugin runtime
    PluginManifest, plugin_manifest, validate_plugin_manifest,   // manifest
    ProviderCapabilities, ProviderCapability,                    // capability
    ChatRequest, ChatResponse, ChatStreamChunk, Usage,           // types
    ProviderError, ProviderResult,                                // error
};
```

子模块内部细节走 `pub(super)` / `pub(crate)`，外部访问统一通过 lib.rs re-export。

## 演进方向（ADR-0001）

| 版本 | 动作 |
|------|------|
| **0.2.1**（当前） | 三巨兽拆分完成；编译期 thin wrapper provider 标 `#[deprecated]`；plugin runtime 与编译期 provider 共存 |
| **0.3.0** | 删除 5 个 thin wrapper（cohere/deepseek/gemini/mistral/ollama）；ChannelRecord 自动 migration 到 plugin preset |
| **0.4.0** | `gate-providers` 收敛为「1 plugin runtime + N preset bundle」；`builtin_fastpath: true` manifest 标志保留性能保险 |

详见 [ADR-0001 Provider 全插件化迁移](../../docs/architecture/decisions/ADR-0001-providers-as-plugin.md)。

## 关键约束

### 错误归一

所有上游错误归一为 `NormalizedProviderErrorKind`：

- `AuthenticationError`（401/403 auth）
- `RateLimit`（429 + Retry-After）
- `ModelNotFound`（404 model missing）
- `Upstream`（5xx retryable）
- `Policy`（vendor safety block）
- `Decode`（malformed body）

### Secret 来源

`channel_keys` envelope encrypted → `secrets: HashMap<String, String>` slot map → plugin runtime 按 manifest `secret_slot` 引用。env fallback 顺序：`KOOIX_CH_<CODE>_KEY` → `KOOIX_API_KEY` → `KOOIX_PLUGIN_SECRET_<SLOT>`。

### Plugin sandbox

- 绝对 URL 默认拒绝（`security.allow_absolute_chat_path: true` 才允许）
- 内网 / metadata host (169.254.169.254 / fd00::/8 / 127.0.0.0/8 等) 默认拒绝
- DNS rebind guard：reqwest resolver + response peer 双重检查
- 出站 allowlist：`security.outbound_allowlist` 是硬门禁

### 测试

```bash
cargo test -p gate-providers                            # unit + integration
cargo test -p gate-providers --test custom_provider     # plugin runtime e2e
cargo bench -p gate-providers --bench routing           # 路由热路径压测
cargo bench -p gate-providers --bench sse               # SSE parser 压测
```

## 文档地图

- [DESIGN.md §4 Channel & Key 池](../../DESIGN.md) — 路由策略与 plugin 整流设计
- [docs/plugin-manifest.md](../../docs/plugin-manifest.md) — Plugin manifest v1 完整规范 + 示例
- [docs/wasm-plugin-abi.md](../../docs/wasm-plugin-abi.md) — WASM ABI vNext 设计稿
- [ADR-0001](../../docs/architecture/decisions/ADR-0001-providers-as-plugin.md) — Provider 全插件化迁移决议
