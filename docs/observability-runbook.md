# Kooix Gate Observability Runbook

本页固定本轮 runtime / billing / usage 重构后的 SLO 指标入口。所有指标从 `GET /metrics` 暴露，Prometheus scrape 即可。

## Gateway pipeline

```promql
histogram_quantile(0.95, sum(rate(gateway_stage_duration_seconds_bucket[5m])) by (le, stage, outcome))
sum(rate(provider_route_decisions_total[5m])) by (provider_type, outcome)
provider_runtime_snapshot_version
```

- `stage=route|adapt|execute|...`：定位热路径慢在哪一段。
- `provider_route_decisions_total{outcome="none|error"}` 突增：优先查 channel health / group binding / model alias。

## Provider health probes

```promql
sum(rate(provider_health_probe_total[5m])) by (provider_type, outcome, status_bucket)
histogram_quantile(0.95, sum(rate(provider_health_probe_duration_seconds_bucket[5m])) by (le, provider_type, outcome))
```

- `outcome=auth_error`：channel key / upstream 认证失效，health checker 会自动 disable active channel。
- `outcome=rate_limited`：探活被 429，默认只记录指标和日志，不改变 channel 状态。
- `status_bucket=5xx|network` 持续升高：上游不可用或网络异常，连续失败达到阈值会进入 auto-disable。
- compile-time provider 使用默认低成本 probe model；channel `supported_models[0]` 会覆盖默认模型，plugin channel 使用 manifest `probe.model` / `max_cost_micros`。

## Billing / usage settlement

```promql
max(billing_outbox_lag_seconds)
sum(rate(billing_outbox_failed_total[5m]))
sum(rate(billing_settle_failures_total[5m])) by (reason)
max(usage_rollup_lag_seconds)
```

- `billing_outbox_lag_seconds` 持续升高：worker 没跑、DB 慢、或 outbox 被锁住。
- `billing_settle_failures_total{reason="pricing_miss"}`：定价规则缺口，不阻断请求但会漏账。
- `usage_rollup_lag_seconds` 持续升高：read model 延迟，会影响 dashboard 新鲜度。

## Worker ownership

```promql
worker_lease_owner
sum(rate(worker_pricing_sync_total[15m])) by (outcome)
sum(rate(worker_inflight_swept_total[5m]))
```

- `worker_lease_owner{job="kooix_pricing_sync"}` 多实例只应一个为 `1`。
- `worker_inflight_swept_total` 突增：可能有大量流式/上游超时导致 pre-debit 过期回退。
