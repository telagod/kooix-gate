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
