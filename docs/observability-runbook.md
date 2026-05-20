# Kooix Gate Observability Runbook

本页固定本轮 runtime / billing / usage 重构后的 SLO 指标入口。所有指标从 `GET /metrics` 暴露，Prometheus scrape 即可。

## Gateway pipeline

```promql
sum(rate(gateway_requests_total[5m])) by (method, path, status_class)
histogram_quantile(0.95, sum(rate(gateway_request_duration_seconds_bucket[5m])) by (le, method, path))
histogram_quantile(0.95, sum(rate(gateway_stage_duration_seconds_bucket[5m])) by (le, stage, outcome))
sum(rate(provider_route_decisions_total[5m])) by (provider_type, outcome)
provider_runtime_snapshot_version
```

- `gateway_requests_total` / `gateway_request_duration_seconds` 是 P1.9 固定命名；旧 `gate_requests_total` / `gate_request_duration_seconds` 暂保留兼容。
- `stage=route|adapt|execute|...`：定位热路径慢在哪一段。
- `provider_route_decisions_total{outcome="none|error"}` 突增：优先查 channel health / group binding / model alias。

## Upstream errors / quota denies

```promql
sum(rate(gateway_upstream_errors_total[5m])) by (kind, provider_type, channel, model)
sum(rate(quota_denies_total[5m])) by (dimension, scope_kind, mode)
```

- `gateway_upstream_errors_total` 按 provider / typed channel / model 展开；fallback provider 使用 `channel="fallback"`。
- `kind=authentication_error|rate_limit_error|model_not_found|policy_error|upstream_error|...` 与 data-plane error shape 同源。
- `quota_denies_total{mode="enforce"}` 只记录硬拦截，dry-run 继续看 `quota_dry_run_total`。

## Trace correlation

P1.9 后 data-plane / billing 链路固定使用同一组低基数 span 和属性，排障时先用 `kooix.request_id` 串起来，再按 org / project / channel / model 收窄。

关键 spans：

- `http.request`：HTTP 入站 span，记录 `request_id`、`status`、`latency_ms`。
- `gateway.data_plane`：chat / responses / embeddings / images / audio handler span。
- `gateway.upstream_request`：每次 upstream provider call / retry attempt span，记录 `operation`、`streaming`、`outcome`、`duration_ms`。
- `billing.emit_usage`：usage 进入 billing outbox 前的 pricing / enqueue span，记录 `outcome=enqueued|pricing_miss|pricing_lookup_error|enqueue_error|not_configured`。
- `billing.outbox.enqueue` / `billing.outbox.fetch_batch` / `billing.outbox.mark_done` / `billing.outbox.mark_failed`：outbox 生命周期 span。
- `billing.consumer.tick` / `billing.consumer.process_one` / `billing.commit_usage`：consumer 批处理、单条 settlement 与 ledger / rollup 落库 span。

核心属性：

- `kooix.request_id`
- `kooix.org_id`
- `kooix.project_id`
- `kooix.api_key_id`
- `kooix.user_id`
- `kooix.channel_id`
- `kooix.group_id`
- `kooix.model`
- `kooix.provider_type`
- `kooix.endpoint`
- `kooix.operation`
- `kooix.streaming`
- `kooix.outcome`
- `kooix.duration_ms`

排障顺序：

1. 用 `kooix.request_id=<uuid>` 找 `http.request`，确认 `status` / `latency_ms`。
2. 跳到同 trace 的 `gateway.data_plane`，确认 `endpoint`、`provider_type`、`channel_id`、`group_id`、`model` 是否符合选路预期。
3. 查看子 span `gateway.upstream_request`：`outcome=error` 时结合 `gateway_upstream_errors_total{provider_type,channel,model}` 判断是否 auth / rate limit / model missing / network。
4. 若客户端成功但账单缺失，沿 `billing.emit_usage -> billing.outbox.enqueue -> billing.consumer.process_one -> billing.commit_usage` 查 `outcome` 与 `outbox_id`。
5. 若 `billing.emit_usage{outcome="pricing_miss"}`，先补 pricing rule；若 `enqueue_error` / `commit_usage failed`，再查 DB / outbox lock / migration。

## Provider health probes

```promql
sum(rate(provider_health_probe_total[5m])) by (provider_type, outcome, status_bucket)
histogram_quantile(0.95, sum(rate(provider_health_probe_duration_seconds_bucket[5m])) by (le, provider_type, outcome))
```

- `outcome=auth_error`：channel key / upstream 认证失效，health checker 会自动 disable active channel。
- `outcome=rate_limited`：探活被 429，默认只记录指标和日志，不改变 channel 状态。
- `status_bucket=5xx|network` 持续升高：上游不可用或网络异常，连续失败达到阈值会进入 auto-disable。
- compile-time provider 使用默认低成本 probe model；channel `supported_models[0]` 会覆盖默认模型，plugin channel 使用 manifest `probe.model` / `max_cost_micros`。

## `least_latency` persistent window

`least_latency` 路由先读 `channel_latency_samples` 的近窗口成功样本均值；若查询失败或无样本，则 fail-open 回退 `ProviderRouter` 内存 `ChannelMetrics`。

```sql
SELECT channel_id, FLOOR(AVG(latency_ms))::BIGINT AS avg_latency_ms
FROM channel_latency_samples
WHERE ts >= NOW() - INTERVAL '5 minutes'
  AND success = TRUE
GROUP BY channel_id
ORDER BY avg_latency_ms ASC;
```

- `source` 只允许 `request` / `health_probe`，避免高基数污染。
- health probe 指标用于 Prometheus 趋势告警；DB 滑窗用于路由决策，不依赖外部 Prometheus query。
- 若表体积增长，先按保留期跑 `ChannelLatencyRepo::prune_older_than`，再考虑分区。

## Fallback hit-rate

Channel Group detail API 会按 `request_events.group_id` 统计近 24h fallback chain 请求分布；控制台 `/admin/groups` 展示 primary / fallback 请求量、fallback hit-rate 与每个节点占比。

快速验尸 SQL：

```sql
WITH chain(group_id, depth) AS (
  SELECT $1::uuid, 0
  UNION ALL
  SELECT cg.fallback_group_id, chain.depth + 1
  FROM chain
  JOIN channel_groups cg ON cg.id = chain.group_id
  WHERE cg.fallback_group_id IS NOT NULL
    AND chain.depth < 5
)
SELECT
  cg.name,
  chain.depth,
  COUNT(re.request_id)::BIGINT AS requests,
  ROUND(
    COUNT(re.request_id)::NUMERIC
    / NULLIF(SUM(COUNT(re.request_id)) OVER (), 0),
    4
  ) AS share
FROM chain
JOIN channel_groups cg ON cg.id = chain.group_id
LEFT JOIN request_events re
  ON re.group_id = chain.group_id
 AND re.ts >= NOW() - INTERVAL '24 hours'
GROUP BY cg.name, chain.depth
ORDER BY chain.depth;
```

- fallback hit-rate = `depth > 0` 的请求量 / 整条 chain 的请求量。
- 若 hit-rate 突升，先看 primary group 的 channel health、rate limit 与 model_filter，再看 upstream auth/rate-limit 错误。
- 若 API 返回 `fallback_stats.has_cycle=true`，说明历史数据或绕过控制面的写入制造了环；控制台更新会阻止新的自引用 / 循环 / 深度超过 5 的配置。
- 旧事件或全局 fallback provider 路径可能没有 `group_id`，不会进入 group hit-rate 统计。

## Channel draining

Draining 用于安全下线 channel / key：先停止新请求，再等待现有 inflight 清空，最后禁用 channel。

操作链：

```bash
curl -X POST "$KOOIX_URL/v1/admin/channels/$CHANNEL_ID/drain" \
  -H "Authorization: Bearer $ADMIN_TOKEN"

curl "$KOOIX_URL/v1/admin/channels/$CHANNEL_ID/drain-status" \
  -H "Authorization: Bearer $ADMIN_TOKEN"

curl -X POST "$KOOIX_URL/v1/admin/channels/$CHANNEL_ID/disable-when-idle" \
  -H "Authorization: Bearer $ADMIN_TOKEN"
```

- `status='draining'` 不进入 `ChannelRepo::list_healthy_in_group`，因此不会接收新 route。
- `drain-status.inflight` 来自当前进程 `ProviderRouter::InflightTracker`，与 `least_conn` 请求生命周期一致。
- `disable-when-idle` 在 `inflight > 0` 时返回 400；等 `safe_to_disable=true` 后再执行即可下线 key/channel。
- 多实例部署时，当前 inflight 视图是进程内 router 计数；后续若要跨实例强一致 drain，需要补 channel-level distributed active request gauge。

## Canary routing

Canary routing 用于把单个 binding 限制在 1%-5% 小流量，并与同组 baseline 自动比较。

控制面字段：

- `channel_group_bindings.canary_percent_bps`
  - `NULL`：普通 binding。
  - `100`：1%。
  - `500`：5%。
- Admin API / UI 当前限制 `100..=500`，避免误把 canary 配成大流量；DB 约束允许 `0..=10000` 以便后续迁移 / 内部工具扩展。
- 路由器使用 deterministic counter gate；未命中的 canary binding 记录 `canary_not_selected` skip trace，并继续走其它可用 channel。

快速验尸 SQL：

```sql
SELECT
  c.code,
  b.canary_percent_bps,
  COUNT(re.request_id)::BIGINT AS requests,
  ROUND(
    COUNT(*) FILTER (WHERE re.status >= 400 OR re.error_code IS NOT NULL)::NUMERIC
    / NULLIF(COUNT(re.request_id), 0),
    4
  ) AS error_rate,
  ROUND(AVG(re.latency_ms)) AS avg_latency_ms,
  ROUND(AVG(re.cost_micros)) AS avg_cost_micros
FROM channel_group_bindings b
JOIN channels c ON c.id = b.channel_id
LEFT JOIN request_events re
  ON re.group_id = b.group_id
 AND re.channel_id = b.channel_id
 AND re.ts >= NOW() - INTERVAL '24 hours'
WHERE b.group_id = $1::uuid
GROUP BY c.code, b.canary_percent_bps
ORDER BY b.canary_percent_bps NULLS FIRST, requests DESC;
```

- 控制台 `/admin/groups` 的 “Canary 对比” 表用同一 24h 窗口展示请求量、错误率、平均延迟、平均成本与 baseline 差值。
- canary 样本量低时不要立即扩大流量；先看 `requests` 是否足够，再看错误率 / latency / cost 是否同时优于 baseline。
- 若 canary 完全无请求，先确认 binding `enabled=true`、channel `active+healthy`、model_filter 命中，以及该 group 正在被 project 使用。

## Billing / usage settlement

```promql
max(billing_outbox_lag_seconds)
max(billing_settle_lag_seconds)
sum(rate(billing_outbox_failed_total[5m]))
sum(rate(billing_settle_failures_total[5m])) by (reason)
max(usage_rollup_lag_seconds)
```

- `billing_outbox_lag_seconds` 持续升高：enqueue 到 worker fetch 的 pending age 在变老，常见是 worker 没跑、DB 慢、或 outbox 被锁住。
- `billing_settle_lag_seconds` 持续升高：outbox 已消费但 settlement / rollup 落库慢。
- `billing_settle_failures_total{reason="pricing_miss"}`：定价规则缺口，不阻断请求但会漏账。
- `usage_rollup_lag_seconds` 持续升高：read model 延迟，会影响 dashboard 新鲜度。

### Ledger reconciliation

P1.5 后 `billing_ledger_events` 是计费审计源，`usage_records` 是控制台 / analytics projection。`actual_settle` 必须能与同一窗口内的 `usage_records` 对齐。

Ledger event types：

- `estimated_debit`：预算 / quota pre-debit 预扣。
- `actual_settle`：请求完成后的实际扣费；`commit_usage` 默认写这个事件。
- `refund`：退款或预扣差额返还。
- `manual_adjustment`：人工调账。
- `invoice_close`：月账单关闭快照。

快速对账 SQL：

```sql
WITH usage_rows AS (
  SELECT request_id, SUM(ROUND(cost_usd * 1000000)::BIGINT)::BIGINT AS usage_micros
  FROM usage_records
  WHERE org_id = $1::uuid AND ts >= $2 AND ts < $3
  GROUP BY request_id
),
ledger_rows AS (
  SELECT request_id, SUM(amount_micros)::BIGINT AS ledger_micros
  FROM billing_ledger_events
  WHERE org_id = $1::uuid
    AND occurred_at >= $2 AND occurred_at < $3
    AND event_type = 'actual_settle'
    AND status = 'posted'
    AND request_id IS NOT NULL
  GROUP BY request_id
)
SELECT COALESCE(u.request_id, l.request_id) AS request_id,
       u.usage_micros,
       l.ledger_micros
FROM usage_rows u
FULL OUTER JOIN ledger_rows l USING (request_id)
WHERE u.request_id IS NULL
   OR l.request_id IS NULL
   OR u.usage_micros <> l.ledger_micros;
```

- `missing ledger`：usage projection 有行但 ledger 无 `actual_settle`，优先查 outbox consumer 与 `billing_settle_failures_total`。
- `orphan ledger`：ledger 有行但 usage projection 缺失，优先查 `commit_usage` transaction 是否中途失败或历史手工写入。
- 月账单状态机在 `billing_invoices`：`draft -> closed -> exported -> paid/waived`；导出归档后用 `POST /v1/orgs/:org_id/billing/:month/state` 推进状态，`exported` 必须携带 `sha256:<hex>` digest。
- CSV 导出响应头 `x-kooix-export-digest=sha256:<hex>`；JSON 导出 `/v1/orgs/:org_id/billing/export.json` 内嵌 `digest`，便于审计留存。
- 成本告警覆盖预算 50/80/100% 阈值；`billing_settle_failures_total{reason="pricing_miss"}` 是 channel/model 单价缺失信号；单请求异常成本先按 24h org spend 粗告警，后续可升级为 request_events P99 / max。

## Quota policy engine

```promql
sum(rate(quota_dry_run_total[5m])) by (dimension, scope_kind, would_deny)
sum(rate(http_requests_total{status="429"}[5m])) by (route)
```

- `mode=dry_run` 规则不扣 Redis、不拦截请求，只在中间件里 peek 当前用量并写 `quota_dry_run_total{would_deny=...}`；若 `would_deny=true` 持续出现，先用控制台 Quota explain 确认命中规则，再切 `enforce`。
- `rpm/tpm` 使用 Redis ZSET sliding window；TPM member 保存本次 estimated token amount。窗口内 member 很多时 explain / dry-run peek 会遍历窗口内 member，异常高 QPS 租户要优先看 Redis CPU。
- `concurrent`、`daily_budget_usd`、`monthly_budget_usd`、`lifetime_budget_usd`、`lifetime_tokens` 使用 Redis counter pre-debit；key 前缀分别是 `qc`、`qb:d`、`qb:m`、`qb:l`、`qtok:l`。
- `lifetime_*` key 不设置 TTL；若误配策略需要人工清零，先导出 `GET /v1/orgs/:org_id/quotas/reconcile` 结果，再按 quota id 精确删除对应 Redis key。
- `GET /v1/orgs/:org_id/quotas/explain` 用于单 scope/model 诊断，返回 `current_used`、`estimated`、`remaining`、`would_deny`、`retry_after_ms`、`reset_at`。
- `GET /v1/orgs/:org_id/quotas/reconcile` 用于对账：Redis counter 是当前 runtime 状态，PG `usage_records` 是 persisted projection；`rpm/concurrent` 属 runtime-only，glob model_filter 的 PG 对账是 best-effort。

## Worker ownership

```promql
worker_lease_owner
sum(rate(worker_pricing_sync_total[15m])) by (outcome)
sum(rate(worker_inflight_swept_total[5m]))
```

- `worker_lease_owner{job="kooix_pricing_sync"}` 多实例只应一个为 `1`。
- `worker_inflight_swept_total` 突增：可能有大量流式/上游超时导致 pre-debit 过期回退。
