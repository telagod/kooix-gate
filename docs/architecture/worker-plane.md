# Worker Plane

Status: active
Scope: outbox consumer、pricing sync、health check/probe、inflight quota recovery、retention 等后台任务。
Last verified: 2026-05-22

> 本页面是 worker plane 的运行时边界与代码锚点。具体后台任务设计见 [DESIGN.md §3-5](../../DESIGN.md)。

## 职责

Worker plane 负责所有 **不应在 HTTP request 链路上执行** 的异步任务：

| 任务 | 频率 | 模块 |
|------|------|------|
| Billing outbox consumer | 持续轮询 | `gate_billing::consumer::OutboxConsumer` |
| Pricing sync（LiteLLM） | 每天 1 次 | `gate_billing::pricing_sync` |
| Health check / probe | 60s | `gate_server::health_check` |
| Inflight quota sweeper | 60s | `gate_server::inflight::InflightSweeper` |
| Channel latency rollup | 5min 滑窗写入 | `gate_storage::channel_latency_samples` |
| Request log retention | dry-run/apply helper | `gate_storage::request_log_retention` |
| Request log monthly partition | dry-run/apply helper | `gate_storage::request_log_partition` |
| Quota usage reconcile | 离线对账任务 | `gate_billing::reconcile::reconcile_usage_ledger` |

## 关键约束

### 实例化

- **不随每个 HTTP replica 重复执行**：worker 子任务通过分布式 lease（PG row lock）锁单实例运行；HTTP 进程可同时跑 worker，也可独立部署 worker-only 模式。
- **`KOOIX_WORKER_ENABLED=false` 可关闭** worker，让 HTTP 实例只服务请求。

### 取消与关闭

- 所有任务 spawn 时持有 `tokio::sync::watch::Receiver<bool>` 作为 shutdown signal。
- 主进程收到 SIGTERM → 广播 shutdown → 任务在当前批次结束后退出 → graceful shutdown 预算 30s。
- 长任务（pricing sync）按 `select! { _ = work => ..., _ = shutdown.changed() => break }` 处理。

### 状态可观测

每个 worker 任务都暴露 Prometheus metrics：

| metric | 含义 |
|--------|------|
| `billing_outbox_lag_seconds` | outbox 最旧未消费事件年龄 |
| `billing_settle_lag_seconds` | usage settle 滞后 |
| `provider_health_probe_total{provider_type,outcome,status_bucket}` | health probe 计数 |
| `provider_health_probe_duration_seconds` | health probe 延迟分布 |
| `inflight_sweeper_recovered_total` | sweeper 退还的 quota 次数 |
| `pricing_sync_last_success_timestamp` | 最近一次成功 pricing sync 时间戳 |
| `quota_denies_total{policy_id,scope,dim}` | quota 拒绝计数 |

### 批量与幂等

- **批量优先**：outbox consumer `enqueue_batch` / `mark_done_batch`；usage records / hourly_rollups / daily_rollups / billing_ledger_events 一次事务批量写。
- **幂等兜底**：`ON CONFLICT DO NOTHING` + `idempotency_key`；duplicate outbox row 安全标记 done。
- **失败重试**：worker 任务失败按 backoff 重试（1s → 5s → 30s → 5min），超过 max retry 写入 dead letter。

## 关键链路

### Billing outbox consumer

```
chat handler → emit_usage event → billing_outbox 表
   ↓
OutboxConsumer.poll()  (每 100ms)
   ↓
batch fetch (limit=100)
   ↓
对账 pricing_rules → compute cost micros
   ↓
SQL transaction:
  - INSERT request_events
  - INSERT usage_records
  - UPSERT hourly_rollups / daily_rollups
  - INSERT billing_ledger_events.actual_settle
  - UPDATE billing_outbox SET status='done'
  - settle quota（多退少补）
```

### Health check / probe

```
HealthChecker.tick()  (每 60s)
   ↓
for each active channel:
  - 按 plugin manifest probe path 或编译期 default model 发轻量请求
  - max_cost_micros=25 限定预算
  - 成功 → 写 channel_latency_samples（kind=health_probe）
  - 失败 → 写 channel_keys 失败统计 → 触发 cooling_down → fallback
```

## 代码锚点

- `crates/gate-server/src/main.rs` — worker 启动入口
- `crates/gate-server/src/worker.rs` — worker 调度器
- `crates/gate-server/src/health_check.rs` — health checker
- `crates/gate-server/src/inflight.rs` — inflight sweeper
- `crates/gate-billing/src/consumer.rs` — outbox consumer
- `crates/gate-billing/src/pricing_sync.rs` — pricing sync
- `crates/gate-billing/src/reconcile.rs` — usage ledger reconcile
- `crates/gate-storage/migrations/` — request_log_events 月分区 / retention

## 跨页面交叉

- 触发任务的 mutation → [Control Plane](./control-plane.md)
- 触发任务的真实流量 → [Data Plane](./data-plane.md)
- 故障处置 → [docs/observability-runbook.md](../observability-runbook.md)
