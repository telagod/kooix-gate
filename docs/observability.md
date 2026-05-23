# Observability — Kooix Gate

> 0.4.34 收尾：Prometheus metrics 命名审计 + Grafana dashboard JSON 模板 + OTLP trace 字段表。

## Prometheus metrics

所有 metric 走 `metrics` crate + `metrics-exporter-prometheus`，默认 expose `:9090/metrics`。

### Request lifecycle

| Metric | Type | Labels | 说明 |
|--------|------|--------|------|
| `gate_chat_requests_total` | counter | `model, provider, status` | OpenAI-compatible 入站请求数 |
| `gate_chat_latency_ms` | histogram | `model, provider` | 完整 chat 链路延迟 |
| `gate_chat_tokens` | histogram | `model, provider, kind` (prompt/completion) | token 用量 |

### Upstream

| Metric | Type | Labels | 说明 |
|--------|------|--------|------|
| `gate_upstream_errors_total` | counter | `channel, status, code` | 上游错误（status 4xx/5xx） |
| `gate_upstream_latency_ms` | histogram | `channel, provider` | 上游 RTT |
| `gate_channel_health_status` | gauge | `channel, health` (healthy/degraded/unhealthy) | 当前健康状态 |

### Routing

| Metric | Type | Labels | 说明 |
|--------|------|--------|------|
| `gate_route_decisions_total` | counter | `strategy, outcome` | 路由决策（priority/weighted_random/...）|
| `gate_route_skip_total` | counter | `reason` | 跳过原因（disabled/draining/canary_miss） |

### Quota

| Metric | Type | Labels | 说明 |
|--------|------|--------|------|
| `gate_quota_denies_total` | counter | `scope_kind, dimension, mode` | 拒绝次数（含 dry_run） |
| `gate_quota_predebit_total` | counter | `dimension` | 预扣次数 |
| `gate_quota_settle_lag_ms` | histogram | `dimension` | 预扣到结算的延迟 |

### Billing

| Metric | Type | Labels | 说明 |
|--------|------|--------|------|
| `gate_billing_settle_lag_seconds` | gauge | `org` | 当前最大结算 lag |
| `gate_outbox_backlog` | gauge | `table` (usage/request_events/rollups) | outbox 待处理数 |

### WASM Plugin (ADR-0003 v0)

| Metric | Type | Labels | 说明 |
|--------|------|--------|------|
| `gate_plugin_wasm_calls_total` | counter | `channel, hook, status` (ok/timeout/oom/panic/no_module/call_error/digest_mismatch/...) | WASM transform 调用数 |

## Grafana Dashboard

模板：[`deploy/grafana/dashboards/kooix-gate-overview.json`](../deploy/grafana/dashboards/kooix-gate-overview.json)

包含 10 个 panel：
- Requests / sec
- p95 Latency
- Upstream 5xx
- Quota Denies
- Request rate by model
- Upstream errors by channel
- **WASM plugin calls (新 0.4.x)**
- Billing settle lag
- Outbox backlog
- Channel health by status

导入：

```bash
# Grafana 界面：+ Create → Import → Upload JSON file
# 或 grafana-cli：
grafana-cli admin export-dashboard \
  --dashboard-file=deploy/grafana/dashboards/kooix-gate-overview.json
```

## OpenTelemetry Trace

启用：`KOOIX_OTLP_ENDPOINT=http://otel-collector:4317`

### Span 字段

每个 chat request 产生顶层 span `kooix.gate.chat`，子 span 包含：

| Span name | 字段 | 说明 |
|-----------|------|------|
| `kooix.gate.chat` | `request_id, org_id, project_id, api_key_id, model, status_code` | 顶层入站 |
| `kooix.gate.route` | `channel_id, channel_code, strategy, attempts` | 路由决策 |
| `kooix.gate.upstream` | `channel_id, provider, latency_ms` | 上游调用 |
| `kooix.gate.billing` | `usage_record_id, cost_micros` | 计费写入 |
| `kooix.gate.wasm` | `channel_id, hook, status` | WASM hook 调用（0.4.x） |
| `kooix.gate.outbox` | `table, batch_size` | outbox 投递 |

### Sampling

默认 head-based sampling，配置：

- `KOOIX_OTLP_SAMPLE_RATE=0.1` (10% 抽样)
- 错误请求 100% 采样（force-on）

## 控制台事故页

`/admin/incidents` 整合下列源：

- top failing channels（gate_upstream_errors_total by channel）
- quota deny top（gate_quota_denies_total）
- upstream 401/429/5xx 分类
- 最近 audit log 危险操作

## Runbook

详见 [observability-runbook.md](./observability-runbook.md)（上游全挂 / Redis 故障 / outbox backlog / pricing sync）。
