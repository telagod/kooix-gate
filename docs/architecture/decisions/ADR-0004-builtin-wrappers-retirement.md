# ADR-0004: 4 大编译期 wrapper 退役（v0.5.0-rc2）

- Status: **Accepted (2026-05-28)** — 收 [ADR-0001](./ADR-0001-providers-as-plugin.md) 最后一条 verification、[ADR-0002](./ADR-0002-fastpath-runtime.md) fastpath 收敛；migration 20260528000001 就位；4 个 wrapper 物理删除留 v0.5.x 执行（详见 [ROADMAP](../../../ROADMAP.md)）
- Deciders: telagod
- Affected: `crates/gate-providers/src/{openai,anthropic,azure,bedrock}.rs`, `crates/gate-providers/src/router/builder.rs`, `crates/gate-server/src/{health_check.rs, routes/admin/{probe,channels,mod}.rs}`, channels DB migration

## Context

[ADR-0001](./ADR-0001-providers-as-plugin.md) 决定全 provider 走 plugin manifest 接入。0.3.0 退役 5 个 thin wrapper（cohere/deepseek/gemini/mistral/ollama），保留 4 个 fast-path wrapper（openai/anthropic/azure/bedrock）作"性能保险"。

[ADR-0002](./ADR-0002-fastpath-runtime.md) 在 0.4.0 落地 `builtin_fastpath` 静态分发，4 大 fast-path provider 走 plugin manifest 接入但 runtime 跳过解释器，性能回到编译期水准（× 0.74-1.00）：

| 路径 | mean | vs builtin |
|------|------|------------|
| `builtin_openai`（编译期 wrapper） | 25.6 µs | baseline |
| `plugin + fastpath`（manifest + 静态分发） | 22-26 µs | × 0.86-1.02 |

也就是说，**编译期 wrapper 的存在不再有性能理由**：
- `crates/gate-providers/src/openai.rs`、`anthropic.rs`、`azure.rs`、`bedrock.rs` 4 个文件 1500+ 行代码。
- `router/builder.rs:39-104` 4 个 `match provider_type` 分支。
- `health_check.rs:731-785` + `probe.rs:60-308` 两套独立的 provider_type → URL/header/body 构造逻辑。
- `admin/mod.rs:506` 硬编码 `compile_time = ["openai", "anthropic", "azure", "bedrock"]`。
- `admin/channels.rs:150-172` 白名单同时列 21 个 provider_type，绝大多数已退役。

这套**双层接入面**违反 ADR-0001 第 1 条决议"plugin manifest 单一接入面"，也是 ADR-0001 verification 最后一条未勾选项（"`gate-providers` 从 9 adapters 收敛为 1 plugin runtime + N 个内置 preset bundle"）。

## Decision

**v0.5.0-rc2 删除 4 大编译期 wrapper，全部走 plugin manifest + fastpath 静态分发。**

### 收口动作

| # | 动作 | 位点 |
|---|------|------|
| 1 | DB migration：channels.provider_type ∈ {openai,anthropic,azure,bedrock} → 'plugin' + 注入 `model_mapping.plugin.preset.provider` | `migrations/20260528000001_retire_builtin_wrappers.sql` |
| 2 | 删 `router/builder.rs` 的 anthropic/azure/bedrock 分支；`_` fallback 从 `OpenAiProvider` 改 `CustomHttpProvider`；legacy 4 种 provider_type fail-loud | `crates/gate-providers/src/router/builder.rs` |
| 3 | 重写 `health_check::build_standard_probe`：用 `plugin_preset` capabilities + manifest path 构造 probe，不再 match provider_type | `crates/gate-server/src/health_check.rs:725-836` |
| 4 | 重写 `admin/probe.rs` 的 provider 实例化：统一走 `CustomHttpProvider` | `crates/gate-server/src/routes/admin/probe.rs:60, 204, 308` |
| 5 | `admin/channels.rs` 白名单缩到 `["plugin", "custom", "http", "http_plugin"]` | 同上 |
| 6 | `admin/mod.rs:list_provider_capabilities` 删 `compile_time` 维度，全部走 plugin_preset | 同上 |
| 7 | 删 `crates/gate-providers/src/{openai,anthropic,azure,bedrock}.rs` | — |
| 8 | `sigv4.rs` 保留（plugin runtime + fastpath 复用） | — |
| 9 | `lib.rs` 移除 `pub mod {openai,anthropic,azure,bedrock}` | — |

### 设计约束

1. **fastpath 自动启用**：migration 后 4 大 channel 的 manifest 命中 `ProviderPresetKind::{Openai, AnthropicMessages, AzureOpenai, BedrockConverse}`，`plugin_manifest::apply_preset` 自动注入 `builtin_fastpath = true`。用户无感、性能无损。

2. **bedrock 凭证 breaking change**：
   - 旧路径：access_key = `channel_keys.key_enc`（明文 = api_key），secret_key = `KOOIX_CH_<CODE>_SECRET` env。
   - 新路径：access_key = secret_slot `aws_access_key`，secret_key = secret_slot `aws_secret_key`。
   - **减痛**：fastpath 加 env 兜底——secret slot 空时读 `KOOIX_CH_<CODE>_ACCESS_KEY` / `KOOIX_CH_<CODE>_SECRET_KEY`（与旧 env 保持类似命名）。CHANGELOG 红字 + upgrade note。

3. **legacy provider_type fail-loud**：migration 后如果还有 channel.provider_type ∈ {openai,anthropic,azure,bedrock}（手工绕过 migration），builder 给出明确 error 引导跑 migration，不静默回退。

4. **回滚路径**：migration 头部注释提供 SQL 回滚一条龙，但 binary 不再带 4 wrapper —— **回滚需同时回 0.4.x binary**。CHANGELOG 红字提示。

5. **测试覆盖**：
   - `cargo test --workspace` 全绿。
   - `gate-providers` 既有 18+ capability matrix golden test 覆盖 manifest runtime。
   - `gate-server` channel/health/probe 测试用 plugin 形式重写。
   - bench `plugin_vs_builtin.rs` 改名 `fastpath_vs_manifest.rs`，对比 fastpath vs 解释器（builtin 已删）。

## Consequences

### Positive

- ADR-0001 verification 最后一条勾选，单一接入面战略落地。
- `crates/gate-providers/` 代码量预计 -1500 行；`router/builder.rs` -100 行；`health_check.rs` -80 行；`probe.rs` -150 行。
- 新增 provider 0 适配代码（已经如此，但白名单/默认值/probe 等旁路全部消失）。
- 测试矩阵单一化：只测 plugin runtime + 4 fastpath，不再维护 4 套 wrapper 测试。
- `gate-providers` 公共 API 缩减：删 `pub mod openai/anthropic/azure/bedrock`，降低下游耦合。

### Negative / Risks

- **bedrock 凭证 breaking**：见上文减痛方案，env 兜底缓解，但仍要 CHANGELOG 提示。
- **migration 不可逆**：binary 删 wrapper 后无法回滚 channel 数据，必须同时回 binary。
- **health_check / probe 重写**：旁路代码改造面积大，需逐 provider 跑 e2e。

### Verification

- [ ] migration 20260528000001 跑 dry-run，旧 channel 数据 fixture 全转 plugin
- [ ] `cargo clean -p gate-storage && cargo test --workspace` 全绿
- [ ] bedrock channel 用新 secret slots 跑 chat 成功
- [ ] bedrock channel 用旧 env 兜底跑 chat 成功（向后兼容）
- [ ] health_check 对 4 大 plugin channel probe 成功
- [ ] CHANGELOG 红字 upgrade note + bedrock secret slot 迁移指引
- [ ] ADR-0001 verification checkbox 勾选

## References

- [ADR-0001 Provider 全插件化迁移](./ADR-0001-providers-as-plugin.md) §"0.4.0 收口" verification
- [ADR-0002 Plugin Runtime Fast-path](./ADR-0002-fastpath-runtime.md)
- [Migration 20260522000001](../../../crates/gate-storage/migrations/20260522000001_migrate_thin_wrapper_to_plugin.sql)（5 thin wrapper 退役模板）
