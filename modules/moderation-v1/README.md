# moderation-v1

Kooix Gate 第一条护城河 v1 WASM transform 模块 —— **关键词 + 类别审核**。
按 [ADR-0007 §3 banned-signal 三层规则](../../docs/architecture/decisions/ADR-0007-banned-signals-v1.md) 给语义层补一份内置词典实现。
按 [ADR-0006 WASM ABI v1](../../docs/architecture/decisions/ADR-0006-wasm-abi-v1-component-model.md) Component Model 编译。

## 它做什么

对 `transform_request` / `transform_response` / `transform_stream_event` 三个 hook
中传入的 JSON / 文本扫描 6 类常见违禁关键词（英文 + 中文），命中即原位替换为
`<moderation:{category}>` 占位符。

| category      | 含义                       |
|---------------|----------------------------|
| `hate`        | 仇恨 / 歧视                |
| `harassment`  | 骚扰 / 欺凌                |
| `self_harm`   | 自伤 / 自杀                |
| `sexual`      | 色情                       |
| `violence`    | 暴力                       |
| `illegal`     | 违法（毒品 / 武器 / 盗窃） |

每个 hook 调用都会在返回的 `metadata` 字段填入 JSON 统计
`{"total":N,"by_category":{"hate":2,"violence":1}}`，供宿主端审计 / 计费消费。

## 匹配策略

- **英文**：每类的关键词合成一条 `(?i)\b(?:k1|k2|...)\b` 单一 alternation 正则，
  长关键词优先；走 `\b` 单词边界，避免 `phate` / `chateau` / `assassin` 之类
  合法词被误伤。
- **中文**：逐 keyword 子串扫描（`str::starts_with` 字节循环），符合中文无
  word boundary 的事实。
- **类别顺序**：`self_harm → hate → harassment → sexual → violence → illegal`。
  先命中的占位符把 raw 吃掉，后续类别不会再扫到原始关键词，避免双重计数。

## Allowlist

请求体可在 JSON 顶层放 `_kooix_allowlist: ["literal1", "literal2"]` 列举要跳过
的字面值（如做学术研究需要保留 "色情" / "porn" 等词）。执行时该字段会被
**取走再 moderate**，所以不会出现在结果里。SSE chunk 路径
（`transform_stream_event`）不支持 allowlist。

## 性能预算（ADR-0007 性能预算章节）

- **目标**：典型 4KB chat payload，host bench p99 < 3ms（release）
- **当前 baseline**：单元测试 `typical_4kb_payload_under_5ms` 在 debug build
  下 < 5ms（release 通常 < 1ms）
- **热路径优化**：所有正则在 `once_cell::Lazy` 中编译一次；每次 transform
  仅 N 次 `find_iter` + 中文 `starts_with` 循环，无额外分配

## 构建

```bash
# host-side 单测 + golden
cd modules/moderation-v1
cargo test --lib

# component-model wasm
cargo build --target wasm32-wasip2 --release
# 产物：target/wasm32-wasip2/release/moderation_v1.wasm
```

## fixtures/ + golden/

| fixture | golden | 类型 |
|---------|--------|------|
| `abuse_request.json` | `abuse_request.json` | 含 hate + harassment 的 OpenAI chat completion 入参 |
| `clean_sse.txt` | `clean_sse.txt` | 完全干净的 SSE chunk（验证无误伤） |

修改规则后若产生预期输出变化，更新 `golden/*`，提交前 review diff。

## 跟 ABI v1 的关系

本模块用 `wit-bindgen` 0.57 + 直接 import `crates/gate-wasm/wit/kooix-plugin.wit`
的 `plugin` world，与 [`pii-redact-v1`](../pii-redact-v1) 同构。

`crate-type = ["cdylib", "rlib"]` 让 host-side 单测复用同一 `lib.rs`；wit-bindgen
生成的 `cabi_post_*` symbol 在 host linker 下会失败（含 `:` 字符），所以
`autotests = false` 关掉 integration test —— 所有测试通过 `cargo test --lib` 集中跑。

## Out of scope

- 自定义词典上传（M6.x 后续 admin UI）
- 多语言（仅英文 + 简体中文常见）
- 上下文相关检测（如 "kill" 仅在含 "myself" 上下文才算 self_harm）—— 留 v2
- 三层规则的另外两层（vendor classifier、LLM judge）—— 本模块只补语义层
