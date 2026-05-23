# gate-billing

计费层：Outbox pattern + 多维度 pricing + LiteLLM 自动同步 + invoice 状态机。

## 关键设计

- **Outbox 解耦**：业务事务和计费写入解耦，支持 `enqueue_batch` / 批量 settlement / 批量 mark done，幂等 `ON CONFLICT DO NOTHING` 兜底重复事件
- **`billing_ledger_events`**：单一审计源，`estimated_debit` / `actual_settle` / `refund` / `manual_adjustment` / `invoice_close` 五种事件
- **多维度 pricing**：按 dimension × conditions 精准匹配，支持缓存折扣 / 批量折扣 / 分层定价
- **Invoice 状态机**：`draft → closed → exported → paid/waived`，导出 digest 绑定审计留存
- **LiteLLM 同步**：可选 worker，按周期从 LiteLLM model registry 拉最新 pricing

## 模块

- `outbox/` — outbox enqueue / consumer / mark done batch
- `pricing/` — `pricing_rules` repository + dimension matcher + condition evaluator
- `ledger/` — billing_ledger_events writer + reconciler
- `invoice/` — 月账单状态机
- `litellm/` — LiteLLM registry sync worker
- `usage/` — `usage_records` projection（trigger from `request_events`）

## 流式计费正确性

`stream_options.include_usage` 强制注入 → 末帧捕获 usage → 写 outbox → worker 批量落 `usage_records` projection + `billing_ledger_events.actual_settle`。详见 [DESIGN.md § 5 流式三段式扣费](../../DESIGN.md)。
