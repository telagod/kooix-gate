# Chaos Testing — Kooix Gate

> Status: **设计稿（0.4.99 占位 → 0.5.x 实装）**
>
> 来源：[archived product-review §5.2](../archive/2026-05-product-reviews/product-review-2026-05-26.md) — "chaos test 缺：限流挂掉 / Redis 闪断 / 上游 503 风暴 / pool 耗尽，没有 deterministic 复现 case"。

## 目标

让"线上某依赖突然挂掉时 gate 行为如何"成为可在 CI 复现的契约，而不是出事故后才知道。

## 不做什么

- 不做生产真流量压测（那是 perf-smoke 的职责）。
- 不做随机故障注入（Netflix Chaos Monkey 风格），让人觉得在赌博。
- 不做不确定性测试（每次跑结果不一样的"chaos"是噪音不是信号）。

## 实施方案

### Phase 1 — 基础设施（0.5.0）

| 依赖 | 故障模式 | 工具 | 期望行为 |
|------|---------|------|---------|
| PostgreSQL | 连接拒绝 | testcontainers 停容器 | quota / billing fail-open 不阻断；audit drop + warn |
| PostgreSQL | 慢查询（5s） | toxiproxy latency | acquire_timeout 触发 → 503 fast-fail |
| PostgreSQL | pool 耗尽 | tokio::join 100 个 sleep(60s) hold | 新 acquire 超时 → 503 |
| Redis | 拒绝连接 | testcontainers 停容器 | rate_limit / quota 走 fail-open（已实装） |
| Redis | 慢命令（500ms） | toxiproxy latency | Lua script 超时 → fail-open，标记 metric |
| Redis | 闪断重连 | toxiproxy enable / disable cycle | fred pool 自愈，无 thrashing |
| 上游 LLM | 503 风暴 | wiremock 100% 503 | retry-after 透传；channel 自动 disable |
| 上游 LLM | 慢响应 | wiremock delay 30s | provider timeout 触发 → 504；inflight pre-debit refund |
| 上游 LLM | 半闭连接（FIN-WAIT） | toxiproxy timeout | reqwest pool 自愈，下次请求新 connect |

### Phase 2 — 自动化（0.5.x）

- `crates/gate-server/tests/chaos/` 目录
- 每个 case 一个 `#[tokio::test]`，sigil 加 `#[ignore]` 避免 `cargo test` 默认跑
- CI 加 `make chaos` target，单独跑 `cargo test --test chaos -- --ignored`
- 验证产出：每个 case 必须断言 metric（如 `gate_quota_check_total{outcome="fail_open"} > 0`），不只是"没 panic"

### Phase 3 — Drill-friendly fixtures

- 把 toxiproxy 容器化，用一致的端口与 admin API 路径
- 提供 `tests/common/chaos_helpers.rs` 把"减慢 PG 5s"封成 `with_pg_latency(5_000).await`
- 把每个 case 的 "predicted blast radius" 写到 case 文档注释，PR review 时方便审

## Coverage targets（0.5.0 收口前）

- [ ] PG 9 case
- [ ] Redis 6 case
- [ ] 上游 LLM 12 case（3 个 provider × 4 种故障）
- [ ] 验收：CI 跑完所有 chaos case ≤ 8 分钟

## 关联

- 现有正向测试：`crates/gate-server/tests/auth_endpoints_e2e.rs` 等 ~10 个 e2e 套件
- 现有 wiremock 用法：`crates/gate-server/tests/channel_plugin_e2e.rs`
- 产品文档：[observability-runbook § 应急响应](../observability-runbook.md)、[security-runbook](../security-runbook.md)

## 决策原因

product-review §5.2 判词：缺 deterministic 复现 case 让"限流挂了""Redis 闪断""上游 503 风暴" 这些事故只能事后复盘，无法事前防御。chaos test 不是"看代码会不会崩"，而是"确认 fail-open / fail-closed / retry / circuit breaker 的契约真的成立"。

v0.4.99 仅设计文档化；实装在 0.5.x（按 Phase 1 → 2 → 3 推进）。
