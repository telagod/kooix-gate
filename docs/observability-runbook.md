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

## Console incident center

平台管理员可从控制台 `/admin/incidents` 或 API `GET /v1/admin/incidents?org_id=<uuid>&hours=24` 查看事故摘要：

- `recent_errors`：近窗口最近错误，来自 `request_events`，缺表时回退 `usage_records`。
- `top_failing_channels`：按错误数 / 错误率 / 最近错误时间排序的失败 channel，关联 `channels.name/provider_type`。
- `quota_denies_top`：与 `quota_denies_total` 同步维护的 process-local runtime snapshot，自服务启动后累计。
- `upstream_error_classes`：持久化请求里的 `401 auth`、`429 rate limit`、`5xx`、其它 `4xx` 与 unknown 分类。
- `upstream_errors_runtime_top`：与 `gateway_upstream_errors_total` 同步维护的 process-local runtime snapshot，自服务启动后累计。

快速 API 验尸：

```bash
curl "$KOOIX_URL/v1/admin/incidents?hours=24" \
  -H "Authorization: Bearer $ADMIN_TOKEN"
```

判读顺序：

1. 先看 `recent_errors`，用 `request_id` 跳到 `/admin/requests` 或 trace 中的 `kooix.request_id`。
2. 再看 `top_failing_channels`：如果单个 channel 错误率高，优先 drain / disable 对应 channel；若 fallback/unknown 高，查 project default group 与 fallback provider。
3. `quota_denies_top` 高时，去 `/orgs/:org_id/quotas` 看命中规则；注意该列表是运行时快照，重启会清零，长期趋势看 Prometheus。
4. `upstream_error_classes.auth_401` 高：优先轮换 channel key；`rate_limit_429` 高：调低 routing weight / 增加 fallback；`upstream_5xx` 高：看 provider status 与 health probe。
5. 多实例部署时 runtime snapshots 是单进程视图；跨实例聚合仍以 Prometheus 为准。

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

## Incident runbooks

以下五条是 P1.9 固定事故处置链。先从 `/admin/incidents` 收敛影响面，再用 Prometheus / SQL / CLI 验证根因；所有变更优先使用 drain / disable / config rollback 这类可逆动作。

### 上游全挂 / no healthy upstream

**Signals**

```promql
sum(rate(provider_route_decisions_total{outcome=~"none|error"}[5m])) by (provider_type, outcome)
sum(rate(gateway_upstream_errors_total[5m])) by (kind, provider_type, channel, model)
sum(rate(provider_health_probe_total[5m])) by (provider_type, outcome, status_bucket)
```

- 客户端错误通常表现为 `no_healthy_channel`、`model_not_found`、上游 `authentication_error` / `rate_limit_error` / `upstream_error`。
- `/admin/incidents` 里若 `top_failing_channels` 多个 channel 同时升高，先按 provider_type / model 判定是上游区域性故障、统一凭证失效，还是 routing group 误配。
- Health checker 行为：`auth_error` 会 auto-disable active channel；`rate_limited` 只记录指标和日志；连续 `5xx|network` 可触发 auto-disable。

**止血**

1. 对单个坏 channel 先 drain，避免新流量继续打过去：

   ```bash
   curl -X POST "$KOOIX_URL/v1/admin/channels/$CHANNEL_ID/drain" \
     -H "Authorization: Bearer $ADMIN_TOKEN"
   curl "$KOOIX_URL/v1/admin/channels/$CHANNEL_ID/drain-status" \
     -H "Authorization: Bearer $ADMIN_TOKEN"
   curl -X POST "$KOOIX_URL/v1/admin/channels/$CHANNEL_ID/disable-when-idle" \
     -H "Authorization: Bearer $ADMIN_TOKEN"
   ```

2. 若某 provider 全挂，进入 `/admin/groups` 把 fallback group 临时切到其它 provider，或在 `/admin/channels` 批量禁用对应 provider 的 unhealthy channel。
3. 若是统一凭证 401/403，先轮换 channel key，再用 `POST /v1/admin/channels/:id/probe` 验证，不要反复扩大重试。
4. 若是 429/rate limit，降低该 channel weight / canary 百分比，增加 fallback 容量；不要让 health probe 误判为禁用条件。

**诊断**

```sql
SELECT id, code, name, provider_type, status, health, last_error, last_error_at
FROM channels
WHERE deleted_at IS NULL
ORDER BY status, health, last_error_at DESC NULLS LAST;
```

```bash
curl "$KOOIX_URL/v1/admin/incidents?hours=1" \
  -H "Authorization: Bearer $ADMIN_TOKEN"
curl -X POST "$KOOIX_URL/v1/admin/channels/$CHANNEL_ID/probe" \
  -H "Authorization: Bearer $ADMIN_TOKEN"
```

**恢复 / 验证**

- channel 恢复后确认 `provider_health_probe_total{outcome="success"}` 回升，`provider_route_decisions_total{outcome=~"none|error"}` 回落。
- 发一条最小 `/v1/chat/completions` smoke，确认 trace 中 `gateway.upstream_request{outcome="success"}` 与 `kooix.channel_id` 命中预期 channel。
- 回滚临时 routing / fallback 改动前，保留 15-30 分钟 canary 或低权重观察窗口。

### Redis 不可用

**Signals**

```promql
sum(rate(quota_denies_total[5m])) by (dimension, scope_kind, mode)
sum(rate(quota_dry_run_total[5m])) by (dimension, scope_kind, would_deny)
```

- 日志关键字：`rate limiter failed; allowing`、`rate quota check failed; fail-open`、`quota counter debit failed; fail-open`、`Redis RPM check failed; fail-open`。
- Redis 主要承载 rate limit / quota 计数：`rpm/tpm` sliding window、budget/concurrent pre-debit、lifetime token/budget counters。
- 当前策略是 Redis 异常 fail-open：data-plane 尽量不中断，但 quota / rate protection 降级，可能出现超额消费。

**止血**

1. 先确认 Redis 是否真的不可达：

   ```bash
   redis-cli -u "$KOOIX_REDIS_URL" ping
   kgctl doctor
   ```

2. Redis 故障期间暂时降低高风险 API key / project 的 quota 配置或 channel group 容量，避免 fail-open 放大成本。
3. 若只影响单实例网络，先重启该实例或迁移流量；若 Redis 集群整体不可用，优先恢复 Redis 持久化与主从，而不是重启所有 gateway。
4. 严禁在未导出 reconcile 前清理 `qb:*` / `qc:*` / `qtok:*` key，避免预算和并发状态无法追账。

**诊断**

```bash
redis-cli -u "$KOOIX_REDIS_URL" info server
redis-cli -u "$KOOIX_REDIS_URL" info stats
redis-cli -u "$KOOIX_REDIS_URL" slowlog get 20
```

```bash
curl "$KOOIX_URL/v1/orgs/$ORG_ID/quotas/explain?scope_kind=project&scope_id=$PROJECT_ID&model=$MODEL" \
  -H "Authorization: Bearer $ADMIN_TOKEN"
curl "$KOOIX_URL/v1/orgs/$ORG_ID/quotas/reconcile" \
  -H "Authorization: Bearer $ADMIN_TOKEN"
```

**恢复 / 验证**

- `redis-cli ... ping` 返回 `PONG`，`kgctl doctor` Redis Lua 检查全绿。
- `quota_denies_total{mode="enforce"}` 与 `quota_dry_run_total` 恢复正常趋势；日志不再出现 fail-open 关键字。
- 对故障窗口内高消费 org 执行 `quotas/reconcile`，必要时通过 billing ledger 做 manual adjustment。

### Postgres 慢查询

**Signals**

```promql
histogram_quantile(0.95, sum(rate(gateway_stage_duration_seconds_bucket[5m])) by (le, stage, outcome))
max(billing_outbox_lag_seconds)
max(billing_settle_lag_seconds)
max(usage_rollup_lag_seconds)
```

- API 侧表现：控制台列表慢、`/admin/incidents` 或 usage dashboard 新鲜度下降、billing outbox lag 抬升。
- Worker 侧表现：`billing_outbox_tick_errors_total`、`billing_outbox_failed_total`、`billing_settle_failures_total{reason="commit_usage"}` 增长。
- 先区分是连接池耗尽、锁等待、缺索引/大表扫描、autovacuum 落后，还是迁移未完成。

**止血**

1. 暂停大范围导出 / 报表 / migration dry-run；控制面限流，保护 data-plane。
2. 若 outbox backlog 同时抬升，可临时增大 worker 批量和缩短 tick 间隔：

   ```bash
   export KOOIX_OUTBOX_BATCH_SIZE=500
   export KOOIX_OUTBOX_INTERVAL_MS=250
   ```

   只在数据库 CPU / I/O 仍有余量时使用，否则会放大锁竞争。
3. 若发现单条 runaway query，先用 `pg_cancel_backend(pid)`；只有确认会长期阻塞且可安全回滚时才 `pg_terminate_backend(pid)`。

**诊断**

```sql
SELECT pid, now() - query_start AS age, wait_event_type, wait_event, state, query
FROM pg_stat_activity
WHERE state <> 'idle'
ORDER BY age DESC
LIMIT 20;
```

```sql
SELECT relname, n_dead_tup, last_vacuum, last_autovacuum
FROM pg_stat_user_tables
ORDER BY n_dead_tup DESC
LIMIT 20;
```

```sql
SELECT relation::regclass AS relation, mode, granted, COUNT(*)
FROM pg_locks
WHERE relation IS NOT NULL
GROUP BY relation, mode, granted
ORDER BY COUNT(*) DESC;
```

- 对疑似慢读模型使用 `EXPLAIN (ANALYZE, BUFFERS)`；优先看 `request_events`、`usage_hourly_rollups`、`channel_latency_samples`、`outbox_events`、`billing_ledger_events`。
- 部署前后跑：

  ```bash
  kgctl migrate --dry-run
  kgctl migrate
  ```

**恢复 / 验证**

- P95 `gateway_stage_duration_seconds` 回落，`billing_outbox_lag_seconds` / `usage_rollup_lag_seconds` 不再持续增长。
- 慢 SQL 修复后补索引或 retention / partition 策略，避免只靠重启止痛。
- 若做过手工 cancel / terminate，把相关 request_id 与 ledger 对账一次，确认没有半落库事件。

### pricing sync 失败

**Signals**

```promql
worker_lease_owner{job="kooix_pricing_sync"}
sum(rate(worker_pricing_sync_total[15m])) by (outcome)
sum(rate(billing_settle_failures_total{reason=~"pricing_miss|pricing_lookup"}[5m])) by (reason)
```

- 日志关键字：`pricing_sync: fetching from LiteLLM`、`pricing_sync: fetched LiteLLM data`、`pricing_sync: complete`、`worker: pricing sync failed`。
- 自动同步从 LiteLLM `model_prices_and_context_window.json` 拉取，HTTP timeout 为 30s；只覆盖 `description LIKE 'litellm/%'` 的 global rules，不覆盖 channel-specific rules。
- 失败不阻断 data-plane，但 `billing.emit_usage{outcome="pricing_miss"}` 会导致请求漏账。

**止血**

1. 确认 worker owner 存在；多实例只应一个 `worker_lease_owner{job="kooix_pricing_sync"} == 1`。
2. 如果外网或 GitHub raw 临时不可达，先在 `/admin/pricing` 或 CLI 手工补关键 model 的 global/channel-specific rules：

   ```bash
   kgctl pricing list --model "$MODEL"
   kgctl pricing set \
     --model "$MODEL" \
     --dimension input_tokens \
     --unit per_million_tokens \
     --rate "$INPUT_USD_PER_MILLION" \
     --priority 100 \
     --description "manual incident input"
   kgctl pricing set \
     --model "$MODEL" \
     --dimension output_tokens \
     --unit per_million_tokens \
     --rate "$OUTPUT_USD_PER_MILLION" \
     --priority 100 \
     --description "manual incident output"
   ```

3. 如果同步数据异常污染了全局规则，先把 `KOOIX_PRICING_SYNC_INTERVAL_SECS=0` 暂停自动同步，再回滚到上一份 pricing snapshot 或手工规则。

**诊断**

```sql
SELECT model, dimension, unit, rate, priority, description, effective_from
FROM pricing_rules
WHERE channel_id IS NULL
ORDER BY effective_from DESC, model, dimension
LIMIT 50;
```

```sql
SELECT model, COUNT(*) AS rule_count, MAX(effective_from) AS newest
FROM pricing_rules
WHERE channel_id IS NULL
GROUP BY model
ORDER BY newest DESC NULLS LAST
LIMIT 50;
```

**恢复 / 验证**

- pricing sync 成功后 `worker_pricing_sync_total{outcome="success"}` 增长，失败计数停止。
- 对事故窗口内 `pricing_miss` 的 model 补跑 usage / ledger 对账；无法自动补账时写 `billing_ledger_events.manual_adjustment`。
- 手工规则若只为止血，恢复自动同步后保留 channel-specific rule 的运营优先级，或删除临时 global rule，避免长期价格漂移。

### outbox backlog

**Signals**

```promql
max(billing_outbox_lag_seconds)
max(billing_settle_lag_seconds)
sum(rate(billing_outbox_failed_total[5m]))
sum(rate(billing_outbox_tick_errors_total[5m]))
billing_outbox_batch_size
```

- backlog 表示 usage 已进入 `outbox_events` 但 worker 未及时 `fetch_batch -> commit_usage -> mark_done`。
- 常见根因：worker 没跑、`KOOIX_MODE` 误设为 `gateway|controlplane`、Postgres 慢、`commit_usage` 失败、poison event 达到 `retry_count >= 3`、旧 worker lease 未过期。
- `fetch_batch` 使用 transaction + `FOR UPDATE SKIP LOCKED` + `locked_until` / `locked_by`；单个卡住 worker 最多等待 lease 过期再被其它 worker 接手。

**止血**

1. 确认至少一个实例以 `KOOIX_MODE=all` 或 `KOOIX_MODE=worker` 运行。
2. 若 DB 健康且 backlog 只是流量峰值，临时提高 worker 吞吐：

   ```bash
   export KOOIX_OUTBOX_BATCH_SIZE=500
   export KOOIX_OUTBOX_INTERVAL_MS=250
   ```

3. 若 `last_error` 指向单类 payload / migration 缺失，先修 payload 或迁移，不要直接删除 outbox 行。
4. 对 `retry_count >= 3` 的 poison rows，导出 payload 与 `last_error` 后再决定重置 retry 或做人工 ledger adjustment。

**诊断**

```sql
SELECT COUNT(*) AS pending,
       MIN(created_at) AS oldest,
       EXTRACT(EPOCH FROM (NOW() - MIN(created_at))) AS oldest_age_seconds
FROM outbox_events
WHERE topic = 'usage' AND processed_at IS NULL;
```

```sql
SELECT id, created_at, retry_count, locked_until, locked_by, last_error
FROM outbox_events
WHERE topic = 'usage' AND processed_at IS NULL
ORDER BY created_at ASC
LIMIT 20;
```

```sql
SELECT retry_count, COUNT(*) AS rows
FROM outbox_events
WHERE topic = 'usage' AND processed_at IS NULL
GROUP BY retry_count
ORDER BY retry_count;
```

```sql
SELECT request_id, COUNT(*)
FROM request_events
WHERE ts >= NOW() - INTERVAL '1 hour'
GROUP BY request_id
HAVING COUNT(*) > 1;
```

**恢复 / 验证**

- `pending` 下降到正常水位，`billing_outbox_lag_seconds` 与 `billing_settle_lag_seconds` 回落，`billing_outbox_batch_size` 不再长时间贴近上限。
- 抽样检查 outbox oldest payload 对应的 `request_events`、`usage_records`、`billing_ledger_events.actual_settle` 都已落库。
- 若做过 retry 重置或手工 adjustment，记录 request_id、outbox id、ledger event id，方便月账单复盘。
