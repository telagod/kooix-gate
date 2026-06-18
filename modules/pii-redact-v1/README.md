# pii-redact-v1

Kooix Gate 第一条护城河 v1 WASM transform 模块 —— **PII Redaction**。
属于 [ROADMAP M6.1](../../ROADMAP.md#m6--v060--合规过滤模块库) 三件套之一，
按 [ADR-0006 WASM ABI v1](../../docs/architecture/decisions/ADR-0006-wasm-abi-v1-component-model.md) Component Model 编译。

## 它做什么

对 `transform_request` / `transform_response` / `transform_stream_event` 三个 hook
中传入的 JSON / 文本扫描 8 类常见 PII，命中即原位替换为 `<redacted:{kind}>` 占位符。

| kind | 触发模式 |
|------|---------|
| `china_id_card` | 18 位国内身份证（含末位 X） |
| `phone_cn`      | 11 位中国大陆手机号（13/14/15/16/17/18/19 段） |
| `email`         | RFC 5322 子集 |
| `openai_key`    | `sk-[A-Za-z0-9_-]{20,}` |
| `anthropic_key` | `sk-ant-[A-Za-z0-9_-]{20,}`（先于 `openai_key` 匹配） |
| `bearer_token`  | `Authorization: Bearer xxx` header 风格 |
| `bank_card`     | 13–19 位连续数字（疑似卡号） |
| `ipv4`          | 标准 IPv4 字符串 |

每个 hook 调用都会在返回的 `metadata` 字段填入 JSON 统计 `{"total":n, "email":a, "phone_cn":b, ...}`，
供宿主端审计/计费消费。

## Allowlist

请求体可在 JSON 顶层放 `_kooix_allowlist: ["literal1", "literal2"]` 列举要跳过的字面值。
执行时该字段会被**取走再 redact**，所以不会出现在 redacted 输出里。SSE chunk 路径（`transform_stream_event`）不支持 allowlist。

## 性能预算（ADR-0007 性能预算章节）

- **目标**：典型 4KB chat payload，host bench p99 < 3ms（release）
- **当前 baseline**：单元测试 `typical_4kb_payload_under_5ms` 在 debug build 下 < 5ms（release 通常 < 1ms）
- **热路径优化**：所有正则在 `once_cell::Lazy` 中编译一次；每次 transform 仅 N 次 `captures_iter`，无额外分配

## Bench

Host-side Criterion benchmark（详见 [`benches/redact.rs`](benches/redact.rs)）：

```bash
cd modules/pii-redact-v1
cargo bench --bench redact            # 完整跑
cargo bench --bench redact -- --quick # smoke / 编译验证
```

链路 / 阈值：

| group | 输入 | 路径 | 目标 (release) |
|-------|------|------|----------------|
| `typical_4kb_payload` | chat_request fixture 扩展至 ~4KB | `redact_json` | p99 < 1ms / p50 < 500us |
| `single_email_string` | `"contact alice@example.com please"` | `redact_text` | p99 < 10us |
| `sse_chunk` | `fixtures/sse_chunk.txt` | `redact_text` | 回归 baseline |

典型 4KB chat payload host-side release p99 < 1ms。

> ⚠ 跟 `autotests = false` 同源的限制：`cargo bench` 走 host link 会触
> 发 wit-bindgen 生成的 `cabi_post_kooix:plugin/...` symbol 写入 lld
> version script 报错。语法/类型校验请用 `cargo check --benches`；
> 真正想跑 bench 需要先临时把 `[lib] crate-type` 切到 `["rlib"]`
> （或单独建一个不带 wit `export!` 的 bench harness crate）。

## 构建

```bash
# host-side 单测 + golden（22 tests）
cd modules/pii-redact-v1
cargo test --lib

# component-model wasm
cargo build --target wasm32-wasip2 --release
# 产物：target/wasm32-wasip2/release/pii_redact_v1.wasm
```

## fixtures/ + golden/

| fixture | golden | 类型 |
|---------|--------|------|
| `chat_request.json` | `chat_request.json` | OpenAI chat completion 入参 |
| `chat_response.json` | `chat_response.json` | OpenAI chat completion 响应 |
| `sse_chunk.txt` | `sse_chunk.txt` | 单个 SSE delta chunk |

修改规则后若产生预期输出变化，更新 `golden/*`，提交前 review diff。

## 跟 ABI v1 的关系

本模块用 `wit-bindgen` 0.57 + 直接 import `crates/gate-wasm/wit/kooix-plugin.wit` 的
`plugin` world，与 [`examples/wasm-transform-v1`](../../examples/wasm-transform-v1) 同构。

`crate-type = ["cdylib", "rlib"]` 让 host-side 单测复用同一 `lib.rs`；wit-bindgen 生成的
`cabi_post_*` symbol 在 host linker 下会失败（含 `:` 字符），所以 `autotests = false` 关掉
integration test —— 所有测试通过 `cargo test --lib` 集中跑。

## Out of scope

- 自定义规则上传（M6.x 后续 admin UI）
- 多语言词典（仅英文 + 简体中文常见）
- 上下文相关检测（如 `card_no:` 标签后才认银行卡）—— 留 v2
- 余额 / quota 度量层
