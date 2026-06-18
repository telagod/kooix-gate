# modules/

Kooix Gate 官方维护的 WASM transform reference 模块（[ROADMAP M6](../ROADMAP.md#m6--v060--合规过滤模块库)）。

按 [ADR-0006 WASM ABI v1](../docs/architecture/decisions/ADR-0006-wasm-abi-v1-component-model.md)
Component Model 编译，目标 triple `wasm32-wasip2`。每个模块独立 crate，
通过主 workspace 的 `[workspace].exclude` 列表解耦，避免污染主 workspace
build cache 与 lock 解析。

## M6.1 三件套

| 模块 | 用途 | 规则数 | 测试 |
|------|------|-------|------|
| [pii-redact-v1](./pii-redact-v1/) | PII 检测脱敏（身份证 / 手机 / 邮箱 / API key / 卡号 / IP） | 8 类 | 22 unit + 3 golden |
| [moderation-v1](./moderation-v1/) | 内容审核（hate / harassment / self_harm / sexual / violence / illegal） | 6 类中英双语 | 31 unit + 2 golden |
| [prompt-injection-v1](./prompt-injection-v1/) | Prompt 注入检测（override / role_swap / exfiltration / encoding / tool_abuse） | 5 类启发式 | 35 unit + 2 golden |

## 设计约定（所有模块共用）

- **入口 ABI**：`wit_bindgen::generate!` 直接 `import` `crates/gate-wasm/wit/kooix-plugin.wit` 的 `plugin` world
- **占位符语义**：命中即原位替换为 `<{module}:{kind}>`，下游 audit 可机械解析
- **统计输出**：`TransformOutput.metadata` 填 JSON `{"total":N, ...}`，宿主侧消费做计费 / 告警
- **Allowlist**：调用方在 JSON 顶层放 `_kooix_allowlist: ["literal1", ...]`，模块取走再扫描；wire payload 上既不会出现 marker 也不会泄露 literal
- **stream chunk 路径**：不支持 allowlist（单 chunk parse 成本不值得）
- **测试结构**：`cargo test --lib` 全跑，**不**用 `tests/`（wit-bindgen 生成的 `cabi_post_kooix:plugin/...` 含 `:` 触发 host linker 名解析失败）；golden 测试内联在 `src/rules.rs::tests`

## 编译产物

```bash
cd modules/pii-redact-v1
cargo build --release --target wasm32-wasip2
# → target/wasm32-wasip2/release/pii_redact_v1.wasm
```

CI 在 [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) 的 `modules-wasm-build` job
内对三模块都跑一次 release 构建。

## 性能预算

- 目标：典型 4KB chat payload host-side release p99 < 1ms（ADR-0007 §性能预算章节）
- 当前 baseline：debug build 内置 `typical_4kb_payload_under_5ms` 单测（< 5ms）
- Criterion bench 模板见 [pii-redact-v1/benches/redact.rs](./pii-redact-v1/benches/redact.rs)；
  实际 release 数字因 wit-bindgen `cabi_post` symbol 名问题受 host link 限制，
  独立 bench crate 留 follow-up（参见 pii-redact-v1 README）

## 加新模块

每个模块按 [pii-redact-v1](./pii-redact-v1/) 模板复制：

```
modules/{your-module}/
├── .gitignore         # target/
├── Cargo.toml         # cdylib + rlib, autotests = false（必须在 [package] 段）
├── src/
│   ├── lib.rs         # wit_bindgen::generate! + Guest impl + take_allowlist
│   └── rules.rs       # 主创意 + once_cell Lazy + Engine + 单元测试 + golden
├── fixtures/          # ≥ 2 个真实样本（JSON + SSE）
├── golden/            # 同名对照
└── README.md
```

新模块同步追加：

1. 主 [`Cargo.toml`](../Cargo.toml) 的 `[workspace].exclude` 列表
2. [`ROADMAP.md`](../ROADMAP.md) 对应 M6.x 段落
3. CI 的 `modules-wasm-build` matrix
