# ADR-0001: Provider 全插件化迁移

- Status: **Accepted (M2 shipped 2026-05-22)** — 0.3.0 binary 与 migration 20260522000001 已落地
- Deciders: telagod
- Affected: `crates/gate-providers/`, `examples/manifest-registry/`, `kgctl plugin`, web Channel UI

## Context

当前 `crates/gate-providers/src/` 同时存在两条 provider 接入路径：

1. **编译期 Provider**（Rust adapter）：9 个 trait impl —— `openai.rs` / `anthropic.rs` / `azure.rs` / `bedrock.rs` / `cohere.rs` / `deepseek.rs` / `gemini.rs` / `mistral.rs` / `ollama.rs`，单文件 60-684 行。
2. **运行期 Plugin**（HTTP manifest）：`custom_provider.rs` + `plugin_manifest.rs` + `plugin_preset.rs`，已支持 18+ preset（OpenAI-compatible / Anthropic / Azure / Vertex AI / Bedrock SigV4 / Gemini / DeepSeek / Mistral / Cohere / Ollama / Groq / Together / OpenRouter / Moonshot / Zhipu / Qwen / Yi / 等）。

两条路径之间的事实：

- `deepseek.rs / mistral.rs / ollama.rs / gemini.rs / cohere.rs` 5 个文件加起来 ≤ 400 行，都是 OpenAI-compatible thin wrapper —— 已被 `plugin_preset.rs` 完全覆盖。
- `azure.rs` / `bedrock.rs` 的差异化逻辑（deployment URL / SigV4 签名）也已经在 plugin manifest auth strategy 里实现。
- ROADMAP 战略主线写明："新增渠道优先不写 Rust adapter，而是写 manifest"。
- `router.rs` 4519 行里有大量分支判断 `is_plugin_provider() / supports_image_runtime() / supports_audio_runtime()`，本质上是因为两条接入路径不统一造成的复杂度。

## Decision

**所有 provider 走 plugin manifest 接入面**。编译期 Rust adapter 不再扩展，分阶段退役。

### 迁移路径

| 阶段 | 版本 | 动作 | 破坏性 |
|------|------|------|--------|
| **0.2.1**（收尾） | 当前 | 文档锁定方向 + ADR + capability parity matrix；新建 channel UI 默认 plugin preset；编译期 provider 标 `#[deprecated]`；router 分支收敛准备 | 否 |
| **0.2.x** | 渐进 | 拆 `router.rs` / `custom_provider.rs` / `plugin_manifest.rs` 三巨兽（见自审 TODO T3.1-T3.3）；plugin runtime 性能基准对齐编译期 provider | 否 |
| **0.3.0**（退役） | 下一 minor | 删除 `cohere.rs / deepseek.rs / gemini.rs / mistral.rs / ollama.rs` 5 个 thin wrapper；保留 `openai.rs / anthropic.rs / azure.rs / bedrock.rs` 作为 fast-path 内置实现，但逻辑上等价于 plugin preset；ChannelRecord 不再支持非 `plugin/custom/http` 的 `provider_type` | **是**：API `provider_type` 收敛 |
| **0.4.0** | 之后 | `gate-providers` 从 9 adapters 收敛为 1 个 plugin runtime + N 个内置 preset bundle；性能 fast-path 通过 manifest 标志位（`builtin_fastpath: true`）走优化路径 | 否 |

### 设计约束

1. **零运行时注册**：内置 preset 走 `plugin_preset.rs` 静态注册，不引入动态 registry / WASM 沙箱；WASM ABI 仍按 `docs/wasm-plugin-abi.md` 走 vNext。
2. **Capability parity 优先**：每个 preset 必须声明完整 `ProviderCapabilities`（chat / streaming / tools / embeddings / image / audio / vision / json_mode / batch），并在 0.3.0 切换前用 capability matrix golden test 锁定。
3. **错误归一**：编译期 provider 与 plugin runtime 共用 `NormalizedProviderErrorKind`，0.3.0 之前完成所有 error mapper 收敛。
4. **Fast-path 保留权**：OpenAI / Anthropic / Azure / Bedrock 等高 QPS provider 可在 manifest 上声明 `builtin_fastpath`，runtime 走优化路径（避免 manifest 解释器开销）。这是性能保险，不是退路。
5. **不破坏存量 channel**：现有 `provider_type=openai|anthropic|...` 的 channel 在 0.3.0 自动迁移为 `plugin` + 对应 preset，DB 加 migration，迁移幂等。

## Consequences

### Positive

- 模块边界清晰：`gate-providers` 从"9 个并列子系统"收敛为"1 个 plugin runtime + preset 静态注册表"。
- 新增渠道无需发版：完全实现 ROADMAP 战略主线。
- `router.rs` 删掉 `is_plugin_provider() / supports_*_runtime()` 分支，行数预计砍 30-40%。
- 测试矩阵简化：plugin runtime 一套测试覆盖所有 provider，capability matrix 集中维护。

### Negative / Risks

- **性能回归风险**：plugin manifest 解释器路径 vs Rust adapter 直调，需 Criterion bench 对比；mitigated by fast-path 标志位。
- **存量迁移风险**：ChannelRecord migration 必须幂等且可回滚；mitigated by dry-run + 双跑窗口期（0.2.x 同时支持两条路径）。
- **错误处理回归**：error mapping 集中到 plugin runtime，需在 0.3.0 切换前覆盖所有上游错误码 fixture。

### Verification

- [x] 0.2.1：ADR + capability parity matrix doc 落地
- [x] 0.2.1：编译期 provider 加 `#[deprecated(note = "use plugin preset; will be removed in 0.3.0")]`
- [x] 0.3.0：删除 5 个 thin wrapper（`cohere.rs/deepseek.rs/gemini.rs/mistral.rs/ollama.rs`）
- [x] 0.3.0：channel migration 20260522000001 自动改 provider_type='plugin' + 注入 preset
- [x] 0.3.0：router builder 对 legacy provider_type fail-loud
- [x] 0.3.0：前端 channel form 收敛 PROVIDER_OPTIONS
- [x] 0.3.0：plugin runtime Criterion bench 已落地（`benches/plugin_vs_builtin.rs`），数据见下表 — **5% 预算超标**，触发 M3 fast-path 立项
- [ ] 0.3.x：capability matrix golden test 覆盖所有 preset
- [ ] 0.3.x：所有 integration test 走 plugin runtime path
- [ ] 0.4.0：`builtin_fastpath` manifest 标志 + 静态分发 fast-path runtime（M3 主线）

### Bench 数据（2026-05-22）

`cargo bench --package gate-providers --bench plugin_vs_builtin`，wiremock localhost endpoint，单次
`Provider::chat()` 调用：

| 路径 | mean | 95% CI |
|------|------|--------|
| `builtin_openai`（编译期 fast-path） | **25.6 µs** | [24.6, 26.7] |
| `plugin_openai_compatible`（manifest runtime） | **36.2 µs** | [35.3, 37.1] |
| **ratio** | **× 1.41** | [× 1.32, × 1.51] |

**结论**：plugin runtime 当前比 builtin 慢约 41%，远超 5% 预算。差距主要来自
manifest 解释器 hot path：placeholder render / endpoint URL template 评估 /
auth header build / 每次 chat 重新构造 `RequestContext`。M3 引入
`builtin_fastpath: true` manifest 标志的必要性由此条 bench 实测支撑，**不是猜测**。

> 复现：`cargo bench --package gate-providers --bench plugin_vs_builtin`，HTML 报告在
> `target/criterion/chat_request/`。

## References

- [自审 TODO](../../stages/2026-05-21-self-critique-todo.md) §3 渠道半成品
- [ROADMAP](../../../ROADMAP.md) 战略主线：渠道插件化
- [HTTP Plugin Manifest 文档](../../plugin-manifest.md)
- [WASM Plugin ABI vNext](../../wasm-plugin-abi.md)
