# ADR-0002: Plugin Runtime Fast-path（M3 v0.4.0）

- Status: **Proposed (M3 kickoff 2026-05-22)**
- Deciders: telagod
- Affected: `crates/gate-providers/src/custom_provider/`, `crates/gate-providers/src/plugin_manifest/`, `crates/gate-providers/src/plugin_preset/`, `examples/manifest-registry/`

## Context

[ADR-0001](./ADR-0001-providers-as-plugin.md) 把所有 provider 收敛到 plugin manifest runtime，0.3.0
已经退役 5 个 thin wrapper。但 0.3.0 验收 bench（[`crates/gate-providers/benches/plugin_vs_builtin.rs`](../../../crates/gate-providers/benches/plugin_vs_builtin.rs)）实测：

| 路径 | mean | 95% CI |
|------|------|--------|
| `builtin_openai`（编译期 fast-path） | 25.6 µs | [24.6, 26.7] |
| `plugin_openai_compatible`（manifest runtime） | 36.2 µs | [35.3, 37.1] |
| **ratio** | **× 1.41** | [× 1.32, × 1.51] |

plugin runtime 比编译期 provider 慢 **41%**，远超 ADR-0001 的 5% 性能预算。这是
manifest 解释器的固有开销，不是实现缺陷：

热路径分析（`custom_provider/mod.rs:235-407`）：

1. **每次 chat() 都重建 `RequestContext`**：`request_context_for(&req)` 把 `ChatRequest`
   转成 `serde_json::Value`，再走 `render_value(template, &ctx)` 把 manifest
   `request.body` 模板里的 `{{model}}` / `{{messages}}` 等 placeholder 渲出来。
2. **endpoint URL 模板评估**：`endpoint_url_with_path_and_context` 跑 placeholder 替换。
3. **auth header build**：`apply_auth_headers` 走 strategy 分发 + HMAC/SigV4 计算
   （Bearer / OAuth / SigV4 / Custom 四条路径都跑 dispatch）。
4. **每次都 clone manifest fields**：`Arc<PluginManifest>` 拿 ref 后还是 clone path。

这些开销对于一般渠道（≤ 100 RPS）完全可以接受，但 **OpenAI / Anthropic / Azure / Bedrock**
作为高 QPS provider，41% 的 latency overhead 在生产规模上累积可观（每 100k RPS
多消耗约 1 个核）。

## Decision

引入 **`builtin_fastpath: true`** manifest 标志位，让 4 个高频 provider 走静态分发优化路径，
代码上相当于编译期 provider 的等价物，但配置层面仍是 plugin manifest 单一接入面。

### 设计骨架

```text
preset.provider = "openai_compatible"
  ↓
manifest.security.builtin_fastpath = true   (静态注册表强制覆盖)
  ↓
CustomHttpProvider::new_with_secret_slots
  ↓ if manifest.builtin_fastpath
  ┌──────────────────────────────────────────────┐
  │ enum BuiltinFastpath {                       │
  │   OpenAi(Arc<OpenAiAdapter>),                │
  │   Anthropic(Arc<AnthropicAdapter>),          │
  │   Azure(Arc<AzureAdapter>),                  │
  │   Bedrock(Arc<BedrockAdapter>),              │
  │ }                                            │
  │ // adapter 不持有 manifest，只读静态 const   │
  │ // chat() 直接走 reqwest::post → json，零模板│
  └──────────────────────────────────────────────┘
  ↓ else
  PluginManifest 解释器路径（保持现状）
```

### 设计约束

1. **manifest 是 source of truth**：用户依旧通过 manifest 配置 channel；fastpath 只是
   runtime 优化，不暴露给 channel UI。
2. **静态注册表不可被 channel 覆写**：`builtin_fastpath` 不在用户 manifest 写，而是
   `plugin_preset.rs` 里的 `ProviderPresetKind::*` 静态注册时强制注入。这样防止
   下游通过 channel.model_mapping 关掉 fast-path。
3. **Capability parity 必须 100% 匹配**：fast-path 实现的 chat / streaming / tools /
   embeddings 行为必须与 manifest runtime 路径**字节级**一致（同一 fixture 必须给同一
   request body / headers / response 解析）。golden test 强制锁。
4. **Fallback 安全**：`builtin_fastpath` 路径出 panic 时不要 take down 进程；
   `catch_unwind` 后降级到 manifest runtime 走一遍（degraded mode），告警上报。
5. **不删 manifest runtime**：fast-path 是叠加层，manifest runtime 仍是默认路径；
   18+ 非 fastpath preset（Cohere / DeepSeek / Mistral / Ollama / Groq 等）只走
   manifest 路径，不需要做 fast-path。

### 验收基准

| 路径 | 目标 ratio (vs 当前编译期等价物) |
|------|----------------------------------|
| `builtin_fastpath = true`（4 个 fast-path provider） | ≤ × 1.02 (2% 预算) |
| `builtin_fastpath = false`（其他 preset） | ≤ × 1.10 (10% 预算) |
| manifest runtime 不退化 | ≤ 当前 0.3.0 × 1.05 |

## Consequences

### Positive

- 高 QPS provider 性能回到编译期水准
- 用户接入面仍是 plugin manifest 单一形态（ADR-0001 战略不变）
- WASM Plugin ABI vNext 可以复用 fast-path adapter 作为 host function 的"标准实现"

### Negative / Risks

- **代码体积增长**：4 个 builtin adapter 文件回归（约 200 行 ×4），但这是性能换体积
- **维护双路径风险**：fast-path 与 manifest 路径可能漂移；mitigated by golden test
- **Fallback 未触发就失效**：catch_unwind 在 async 里捕获覆盖率有限；mitigated by
  optimistic fast-path + pessimistic test fixture 双跑

### Verification

- [x] 0.3.x：`SecurityConfig::builtin_fastpath` 字段 + `plugin_preset.rs` 静态注入 + 用户字段强制清零（4 个新单元测试锁定）
- [x] 0.3.x：capability matrix golden test 覆盖 4 个 fastpath × 9 capability + 23 个 preset（`tests/capability_matrix.rs`）
- [x] 0.3.x：`CustomHttpProvider` 内部 OpenAI fast-path dispatch 落地（chat / chat_stream / embed），3 个集成 test 锁路径正确性
- [x] 0.3.x：`CustomHttpProvider` 内部 Anthropic Messages fast-path dispatch 落地（chat / chat_stream），2 个集成 test
- [x] 0.3.x：`CustomHttpProvider` 内部 Azure OpenAI fast-path dispatch 落地（chat / chat_stream / embed），2 个集成 test（deployment URL + api-version override）
- [x] 0.3.x：catch_unwind fallback 兜底（`run_fastpath` helper），3 个单元测试 + 集成 test 验证 panic 时降级到 manifest runtime
- [ ] 0.4.0：Bedrock SigV4 修真 + fast-path（**编译期 BedrockProvider 当前的 SigV4 是占位**，先修编译期再做 fast-path；详见下方笔记）
- [ ] 0.4.0：preset bundle 拆 crate 评估（`gate-presets-openai` 等可选 feature）

### Bedrock 单独说明

回看 [`crates/gate-providers/src/bedrock.rs:69`](../../../crates/gate-providers/src/bedrock.rs#L69)
的 `sign_request`：写着 *"Simplified: in production this would use proper AWS SigV4 signing"*
—— 它只发 `X-Amz-Access-Key/Secret-Key` 两个头，不是 AWS 标准 SigV4。

而 plugin runtime 在 [`crates/gate-providers/src/custom_provider/sigv4.rs`](../../../crates/gate-providers/src/custom_provider/sigv4.rs)
里**已经实现了完整 SigV4**（manifest auth strategy = AwsSigv4，bedrock_converse preset 自动启用）。

结论：Bedrock 的 fast-path 不是性能问题，是**功能问题**：
1. **当前生产路径**：用户应该走 plugin runtime（auth_strategy=aws_sigv4），不要走编译期 BedrockProvider。
2. **fast-path 前置条件**：先把 `bedrock.rs::sign_request` 改成真实 SigV4（复用
   `custom_provider/sigv4.rs` 的 helper），再做 fast-path 才有意义。
3. **0.3.x 不做**：channel migration 已经把 `provider_type='bedrock'` 全部迁到
   plugin runtime，没有生产 channel 走到编译期 BedrockProvider 上。fast-path
   做了反而是降级。0.4.0 一起干掉编译期 BedrockProvider 或修真。

### Bench 数据更新（2026-05-22 OpenAI fast-path 接通后）

| 路径 | mean | 95% CI | vs builtin |
|------|------|--------|------------|
| `builtin_openai`（编译期 fast-path） | 24.1 µs | [23.5, 24.8] | × 1.00 |
| `plugin_openai_compatible`（manifest runtime） | 35.0 µs | [34.2, 35.7] | × 1.45 |
| `plugin_openai_fastpath`（ADR-0002 dispatch） | **23.1 µs** | [22.3, 24.1] | **× 0.96** |

**结论**：OpenAI fast-path 与 builtin 性能等价（bench 抖动落 × 0.96，CI 上限 × 1.00）。
ADR-0002 的 ≤ × 1.02 预算达成。manifest runtime 仍维持 × 1.45（预期，未被 fast-path 影响）。
复现：`cargo bench --package gate-providers --bench plugin_vs_builtin`。

## References

- [ADR-0001 Providers as Plugin](./ADR-0001-providers-as-plugin.md)（M3 起源 + bench 数据）
- [HTTP Plugin Manifest 文档](../../plugin-manifest.md)
- [WASM Plugin ABI vNext](../../wasm-plugin-abi.md)
- [Bench 源码](../../../crates/gate-providers/benches/plugin_vs_builtin.rs)
