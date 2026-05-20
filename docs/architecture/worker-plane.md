# Worker Plane

Status: active
Scope: outbox consumer、pricing sync、health check/probe、inflight quota recovery 等后台任务。
Last verified: 2026-05-21

## 职责

Worker plane 负责：

- billing outbox consume
- pricing sync
- health check / probe
- inflight sweeper
- lease / cancellation / graceful shutdown

## 关键约束

- 不随每个 HTTP replica 重复执行。
- 任务必须可取消。
- 任务状态必须可观测。
- 批量落库与幂等处理优先。

## 代码锚点

- `crates/gate-server/src/worker.rs`
- `crates/gate-server/src/health_check.rs`
- `crates/gate-server/src/inflight.rs`
- `crates/gate-billing/src/consumer.rs`
