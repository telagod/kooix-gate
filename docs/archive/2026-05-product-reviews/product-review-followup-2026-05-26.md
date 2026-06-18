# Product Review Followup · 批判性自审

> Status: **0.4.102（2026-05-26）启动 第二刀**
> 视角：吾对 v0.4.65-0.4.101 第一刀 37 patch **自我批判**——揭"伪完成"，给真图纸。
> 关联：[product-review-2026-05-26.md](./product-review-2026-05-26.md) | [CHANGELOG.md](../../../CHANGELOG.md)

---

## TL;DR — 一句话定性

**"第一刀疾，但有粉饰"**：37 patch 中真正改 runtime 行为的约 **15 个**（A1-A5 + retry + pool + WASM host_log/record_metric/cwasm + capabilities endpoint + Anthropic/Bedrock extra + Usage lift），其余 22 patch 是文档化 / 测试 / sanity / 占位 env / TODO 注释 / 阶段口号。

把这些当 "20% 文档 + 80% 实装" 是诚实表述，但 README 与 RELEASE.md 把它们都列成"已收口"是**粉饰**。

---

## 🩸 第一类 · 假步骤命名（"step 1/N" 但只做了 step 1）

| 项 | patch | 真实状态 | 误导点 |
|----|------|---------|-------|
| admin.rs 拆分 | 0.4.72 | 仅 pricing 内联 mod；invitations/groups/users/sso 4 大块 4055 行**全没动** | CHANGELOG 写 "step 1/4"，让人以为有 step 2/3/4 在路上 |
| channels page 拆分 | 0.4.76 | 仅抽 6 个静态常量到 `_lib`（53 行）；createForm/editForm state、modal 协调、API call 全在 page 内 | 主体 god page 1199 行实际**没拆**，只少了 53 行 |
| WASM host fn 三件套 | 0.4.80-0.4.82 | host_log + host_record_metric 真实装；**host_get_secret_slot 完全没做**（涉及 manifest secret_slot 声明 + audit hook + channel context 传到 wasm host） | CHANGELOG 写 "B3a step 1+2/3"，step 3 拖到下版本 |
| DataTable 虚拟化 | 0.4.85 | 仅加 maxHeight + sticky head 两个 prop（21 行 diff）；真正 windowing virtualize（row recycle）未做 | CHANGELOG 写 "B4 step 1/3"，admin/requests 万行表仍会卡 |

**修正方向**：要么真做 step 2/3/4（v0.4.109 / 0.4.110 / 0.4.116 / 0.4.117 是这次的）要么明示"step 1 之外是下迭代工作，本版本不会再推"。

---

## 🩸 第二类 · 占位算实装（env 文档化但 runtime 没接）

| 项 | patch | 真实状态 |
|----|------|---------|
| `KOOIX_REQUEST_LOG_BUFFER_SIZE` | 0.4.97 | `.env.example` 写了 3 个 env，runtime 完全没读 / 没生效；CHANGELOG 主动写"本版本仅文档化保留" — 这是诚实，但**整版只做这一件事**包装成 patch 显得稀释 |
| `chat e2e bench` | 0.4.98 | benches/hot_paths.rs 顶部加了 4 行 doc-comment TODO；没新增任何 #[bench] 函数 |
| `chaos-testing.md` 设计稿 | 0.4.99 | 84 行 markdown 的 27 case 矩阵；零代码 |
| Playground capability 联动 | 0.4.90 | 仅把 playground.md 路线第 1 项标 "[x] Backend"；frontend 接入 endpoint 完全没动 |

**修正方向**：把"纯文档 patch" 与 "runtime patch" 在 CHANGELOG 顶部用 `**Type:** docs` / `**Type:** runtime` 标签区分，让 CHANGELOG reader 一眼分辨。

---

## 🩸 第三类 · 漏网（第一刀报告里就该列但漏了）

### 3.1 OpenAI/Anthropic check_status retry-after 解析仅 u64 秒

```rust
// crates/gate-providers/src/openai.rs:236
.and_then(|s| s.parse::<u64>().ok())
.map(|s| s * 1000);
```

HTTP `Retry-After` 头按 RFC 7231 接受**两种格式**：
- `Retry-After: 120`（秒数）
- `Retry-After: Wed, 21 Oct 2026 07:28:00 GMT`（HTTP-date）

后者会被 `parse::<u64>()` 直接 fall through 成 `None`，retry 用默认 backoff 而非上游指定时间——可能在云厂商服务降级时**比上游建议更早重试**，二次冲击。

**v0.4.103 修**。

### 3.2 lift_openai_usage_details 仅提两字段

OpenAI 的 `prompt_tokens_details` 还有 `audio_tokens`；`completion_tokens_details` 还有 `accepted_prediction_tokens` / `rejected_prediction_tokens`（speculative decoding 模型，4o-realtime / o1 系）。

```rust
// 当前只覆盖 cached_tokens 与 reasoning_tokens
if let Some(cached) = prompt_details.get("cached_tokens").and_then(|x| x.as_u64()) { ... }
if let Some(reasoning) = comp_details.get("reasoning_tokens").and_then(|x| x.as_u64()) { ... }
```

`audio_tokens` / `accepted/rejected_prediction_tokens` **进了 raw 但顶层 Usage 没字段**。billing 拿不到。

**v0.4.104 修**：`Usage` 加 `audio_tokens / accepted_prediction_tokens / rejected_prediction_tokens` 三字段 + lift。

### 3.3 SharedHttpClient LRU=8 + clear-all eviction 雷暴风险

```rust
// lib.rs:91
if cache.len() >= SHARED_CLIENT_CACHE_LIMIT {
    cache.clear();
}
```

eviction 策略是 **clear all 8 个 client**（懒省事）。如果有 9 个不同 timeout 桶（罕见但可能：plugin manifest `request.timeout_ms` 自定义会扩散维度），**每来一个新 timeout 都触发全 cache 清空**——所有 channel 重连，雷暴。

`SHARED_CLIENT_CACHE_LIMIT=8` 也没文档说"超出后会发生什么"。

**v0.4.105 修**：改 LRU per-key eviction（删最久未用的一个，不是清空）+ 加 metric 验证。

### 3.4 chat handler metrics 埋点未端到端验证

0.4.66 加了 4 个 metric 函数 + 单测验证函数能 emit；**但 chat handler 4 个出口（流式 ok/error / 非流 ok/error）真的都调了吗？** 没 integration test。

只有单测验证 `record_chat_request` 函数本身能写入 prometheus handle，没验证 handler 跑完后真有 metric 落地。

**v0.4.106 修**：加端到端 test，模拟 chat 请求后 grep render output 含 `gate_chat_requests_total` 4 种 outcome 组合。

### 3.5 metric 名字符串散在多处

```rust
// metrics.rs
metrics::counter!("gate_chat_requests_total", ...)
// chat.rs (调用 record_chat_request 隐藏了 name)
// observability.md (硬编码列出 name)
// Grafana dashboard JSON (硬编码 query name)
```

任何 typo（如把 `gate_chat_requests_total` 写成 `gate_chat_request_total`）只能在 PR review / 运维抓 bug 时发现。

**v0.4.107 修**：抽 `metrics::names::CHAT_REQUESTS_TOTAL` 等 const，所有地方引用同一 const。

---

## 🩸 第四类 · 内联 mod 是"假拆分"

`v0.4.72` 把 pricing 块包到 `mod pricing { use super::*; }`，admin.rs **行数 +13（4235 → 4248）**——CHANGELOG 自己也承认 "行数小增"。

理由是"逻辑边界清晰，方便未来真正拆 admin/pricing.rs"。但：

1. 主 router 仍要写 `pricing::list_pricing_rules`，与原 `list_pricing_rules` 调用方式区别仅是路径前缀；
2. `mod pricing` 内 `use super::*` 是把整个父作用域 import 进来，**符号可见性反而更松**（任何 helper / type 都能 `use`，不需要显式声明依赖）；
3. 真拆到独立文件 `routes/admin/pricing.rs` 需要确切列出依赖，本步没做。

**等价于：把代码挪到一个 `{ }` 里，伪解耦。**

**v0.4.109 / 0.4.116 / 0.4.117 修**：真拆 invitations / groups / sso 到独立文件。

---

## 🩸 第五类 · 文档与代码不同步残留

虽然第一刀做了多轮 doc 同步（0.4.73 / 0.4.89 / 0.4.91 / 0.4.92 / 0.4.93 / 0.4.95），仍有：

### 5.1 Grafana dashboard panel 没补

`deploy/grafana/dashboards/kooix-gate-overview.json` 还是 0.4.34 的 panel set，**没有 `gate_chat_*` panel**。observability.md 列了新指标但 dashboard 用不上。

**v0.4.112 修**。

### 5.2 SECURITY.md 是否完整？

```bash
$ wc -l SECURITY.md
# (待查)
```

vulnerability disclosure 流程、安全联系人、response SLA 是否齐？开源项目 GitHub 期望有规范的 SECURITY.md。

**v0.4.118 检查 + 完整化**。

### 5.3 RELEASE.md "已完成检视表" 是粉饰

11 行表格，每项都打 ✓ —— 但混了"真实装"和"仅文档化"。运维拿这表评估"是否能上 rc1"会被误导。

**v0.4.119 修**：表格按"runtime / docs / partial" 三色分类。

---

## 🩸 第六类 · stream_safe API 是"幽灵 API"

`RetryConfig::stream_safe()` 在 0.4.70 加，CHANGELOG 写"流式路径用此 config 显式表达非幂等"——**但 chat.rs 流式分支根本不调 with_retry**。

整个 codebase **零调用** `stream_safe()`。它是个能编译通过、有单测、运行时永不执行的"装饰性 API"。

```bash
$ rg 'stream_safe\(\)' crates/
# 仅 retry.rs 自身的定义 + 测试，无业务调用
```

**v0.4.108 修**：要么 chat.rs 流式真用，要么去掉 API 假象（保 ::default() 即够）。

---

## 第二刀路线（v0.4.103-0.4.120）

按"批判 → 修复"映射：

| Patch | 类型 | 修哪条 | 优先级 |
|------|------|------|------|
| 0.4.103 | runtime | §3.1 Retry-After HTTP-date | P0 |
| 0.4.104 | runtime | §3.2 Usage lift 覆盖面 | P0 |
| 0.4.105 | runtime | §3.3 SharedClient LRU 雷暴 | P1 |
| 0.4.106 | test | §3.4 chat metrics e2e | P1 |
| 0.4.107 | refactor | §3.5 metric name const | P2 |
| 0.4.108 | runtime | §6 stream_safe 真用 | P1 |
| 0.4.109 | refactor | §1 admin invitations 真拆 | P1 |
| 0.4.110 | refactor | §1 channels form 工厂 | P2 |
| 0.4.111 | design | §1 host_get_secret_slot 设计 | P1 |
| 0.4.112 | docs | §5.1 Grafana dashboard | P2 |
| 0.4.113 | runtime | §2 request_logs buffered trait | P1 |
| 0.4.114 | refactor | §1 channels list state store | P2 |
| 0.4.115 | design | §1 DataTable virtualize 设计 | P2 |
| 0.4.116 | refactor | §1 admin groups 真拆 | P1 |
| 0.4.117 | refactor | §1 admin sso 真拆 | P1 |
| 0.4.118 | docs | §5.2 SECURITY.md | P1 |
| 0.4.119 | docs | §5.3 README/ROADMAP 真实进度 | P0 |
| 0.4.120 | release | 阶段大版收口 + tag | P0 |

P0 修必须在本轮做完；P1 推到本轮之后下迭代可接受；P2 是 nice-to-have。

---

## 第三类后果 — "技术债诚实账"

第一刀完成后剩余真实债务（不算第二刀已规划项）：

1. **playground frontend capability 联动** — backend ready，frontend 还没接
2. **WASM module blob store + auto-mount** (G-002) — 完全没起
3. **WASM ABI v1 wit-bindgen** (G-103) — 完全没起
4. **AssemblyScript SDK npm publish** (G-101) — 本地 package 已有，未发
5. **SCIM v2** (G-105) — 评估完成，未实装
6. **Web bundle 220 → 180 KB** (G-106) — 220 已通过 budget，180 是 stretch goal
7. **chat e2e bench 真实装** — 0.4.98 仅 TODO
8. **chaos test runtime** — 0.4.99 仅设计稿

这些进 v0.5.0 主线，不在 0.4.x patch 范围。

---

## 0.4.113 误判更正 · request_logs buffered 不是真问题

**原 product-review-2026-05-26.md §1 P1-3 写**：

> request_logs 表写入策略：每次请求都同步写？批量？是否有 buffered writer

**实际情况**（0.4.113 复审）：

1. `request_events` 是 canonical 主表（outbox 路径，billing.emit_usage 异步入）
2. `request_log_events` 是 read 投影表（migration `20260520000007` 加的月度分区表）
3. 两个 `RequestLogRepo` trait 实际**只读**（list / find / dashboard_stats / incident_summary / partition 管理），**没有 insert/write 方法**
4. 真实写路径：billing outbox consumer 在 worker plane 异步 batch 入库

**结论**：原 review 把 `RequestLogRepo` 看成同步写 trait 是望文生义。架构已经是**双表 + outbox 异步**——本来就 buffered。

`KOOIX_REQUEST_LOG_BUFFER_SIZE` 等 0.4.97 占位 env 因此**不是必要**——除非要给 read 投影路径加缓冲（但那场景下读路径已是异步 dashboard_stats，无意义）。

**0.4.113 撤回此条 followup 项**。0.4.97 的 env 占位也撤回（在 v0.5.x 时如果真要做 buffered 再启用，但目前没有需求）。

---

*Reviewer: 邪修红尘仙 / Date: 2026-05-26 / 关联 commit: 0a82eb3..427c1cf*
