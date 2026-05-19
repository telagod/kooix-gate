# Kooix Gate 架构 / 性能 / 数据库重构 TODO 审计

Status: implementation pass applied
Scope: 当前 `kooix-gate` 仓库对照 foxnio 同日重构审计坑位的巡检。
Last verified against code: 2026-05-19
Source of truth: 本文基于当前工作区源码、SQL migration、CI workflow、README/DESIGN/ROADMAP 实证盘点；参考文档只作风险维度，不照搬结论。

## Implementation status — 2026-05-19 pass

本文件前半部分保留了最初审计证据，用于解释为什么要斩这些坑；本节记录本轮已经落地的修复，避免后续读者把旧证据误当当前状态。

| Area | Landed artifact | Status |
|---|---|---|
| Gateway / Control Plane / Worker modes | `crates/gate-server/src/modes.rs`、`app.rs` router split、`worker.rs`、`tests/runtime_modes.rs` | done |
| Background jobs | 删除旧 `health_probe`，统一到 `health_check`；outbox/pricing/inflight/health_check 只在 `all|worker` 跑，worker loop 支持 cancellation | done |
| Outbox concurrency | migration `20260519000001_outbox_worker_leases.sql`；`PgOutboxRepo::fetch_batch` 使用 transaction + `FOR UPDATE SKIP LOCKED` + lease | done |
| Request ID / idempotency | `KooixRequestId` extension 贯通 quota inflight、billing outbox、usage settlement；`UsageEvent.idempotency_key` 兼容旧 payload | done |
| Usage read model | migration `20260519000002_request_events_rollups_ledger.sql`；`commit_usage` 双写 `request_events` / rollups / `billing_ledger_events` | done |
| Read path | request log / usage stats 优先读 `request_events` 与 `usage_hourly_rollups`，旧 `usage_records` 保持兼容 fallback | done |
| Route manifest | `crates/gate-server/src/route_manifest.rs` + `scripts/check-route-manifest.mjs`，gateway data-plane route CI gate | done |
| Route manifest export / typed client seed | `GET /route-manifest.json`、`scripts/generate-route-types.mjs`、`web/src/lib/api/route-manifest.ts` 覆盖 82 条 served routes | done |
| CI gates | `.github/workflows/ci.yml` 增加 DB migration gate、security smoke、quality gate、route manifest、bundle budget | done |
| Gateway pipeline contracts | `crates/gate-server/src/gateway.rs` 定义 `GatewayEvent` / `MeteringEvent` / `GatewayStage` / `FailurePolicy`；chat/embeddings/images/audio 接入 stage metrics | done |
| Provider route trace / runtime snapshot | `RouteDecisionTrace` / `ProviderRuntimeSnapshot` / replaceable versioned snapshot metadata / candidate+fallback+selection trace；server 会把 `resolved_model` 写回 upstream request | done: repo-backed route path still supported; compiled snapshot metadata ready |
| Usage storage plan | `kgctl usage-storage plan [--partition|--timescale]` 输出分区 / Timescale / retention dry-run SQL，不把 Timescale 作为默认硬依赖 | done |
| Perf smoke | `crates/gate-server/tests/perf_smoke.rs` + `scripts/perf-smoke.mjs` 覆盖 `/v1/models`、`/v1/chat/completions`、`/v1/admin/dashboard-stats`、`/metrics` | done |
| SLO metrics/runbook | `gateway_stage_duration_seconds`、`provider_route_decisions_total`、`billing_settle_failures_total`、`usage_rollup_lag_seconds` 等指标 + `docs/observability-runbook.md` | done |
| Large-file quality | `scripts/quality-gate.mjs` + `docs/waivers/quality/2026-05-19-large-files.md` 阻断新增未登记 warning；拆分按 waiver exit plan 渐进执行 | accepted with waiver |

Current verification snapshot:

```bash
git diff --check
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && npm run check && npm test && npm run build && npm run bundle:budget
node scripts/quality-gate.mjs
node scripts/check-route-manifest.mjs
node scripts/generate-route-types.mjs --check
node scripts/perf-smoke.mjs
KOOIX_DATABASE_URL=postgres://... cargo run -p kgctl -- migrate --dry-run
KOOIX_DATABASE_URL=postgres://... cargo run -p kgctl -- migrate
KOOIX_DATABASE_URL=postgres://... cargo run -p kgctl -- migrate --dry-run
cargo run -q -p kgctl -- usage-storage plan
cargo run -q -p kgctl -- usage-storage plan --timescale
```

## 0. 审计结论

`kooix-gate` 没有 FOXNIO 那种「旧表 / 新表双轨套餐模型」问题，也没有 Go 单体里 `globalCtx` / `GetDefaultSettingsManager()` 一类硬全局配置单例；但它已经出现同源风险：**data plane、control plane、billing/outbox、provider routing、health jobs、usage analytics 仍在同一个 `gate-server` runtime 和同一个 `AppState` 聚合里合流**。

当前最该避免的不是立刻微服务化，而是先把现有 Rust modular monolith 收束成更清晰的 **gateway / controlplane / worker 三层边界**：

1. `/v1/chat|embeddings|images|audio|models` 热路径只读 runtime snapshot，少碰 admin/control DB 写路径。
2. 管理面 `/v1/admin/*`、settings、channels、keys、quotas、billing CRUD 明确归 control plane。
3. health check、health probe、pricing sync、outbox consumer、inflight sweeper 全部 worker 化，并有 lease / graceful shutdown / metrics。
4. usage_records 不再同时承担请求日志、dashboard 聚合、billing projection、性能分析所有读模型。

一句话：**底盘比 FOXNIO 清爽，但若继续加功能，最先裂的是 runtime 边界、计费落库可靠性、usage 查询性能和工程质量门禁。**

## 1. 对照参考文档的风险判定

| 参考坑位 | Kooix Gate 当前判定 | 关键证据 | 处理优先级 |
|---|---|---|---|
| Data Plane / Control Plane 合流 | 存在中高风险 | `build_router` 把所有 `/v1/*` 叠同一 middleware；`v1_router` 合并 me/settings/models/chat/admin/billing/usage/channels/quotas | P0 |
| handler/provider/settings 全局单例 | 低风险，但 `AppState` 过宽 | 未见默认 settings 全局单例；但 `AppState` / `Repos` 聚合所有依赖 | P1 |
| Gateway pipeline 隐式中间件链 | 存在 | `rate_limit -> quota -> rls` 顺序靠 `app.rs` layer 顺序维持；chat handler 内部再做 provider routing、settle、billing emit | P0 |
| 套餐/计费 source of truth 双轨 | 未见 FOXNIO 式 plans 双轨 | 当前主要是 `pricing_rules` / `model_pricing` legacy compat 与 `usage_records` projection；不存在 subscription/prepaid 旧新双轨 | P2 |
| 扣费 / usage 事件幂等与并发 | 存在高风险 | outbox fetch 未 `FOR UPDATE SKIP LOCKED`；Consumer 未在生产 main 启动；billing emit warn-only 且 request_id 另生成 | P0 |
| migration-first 门禁 | 部分已做，CI 不足 | 使用 `sqlx::migrate!` 版本化 SQL；但 CI 未显式起 Postgres 做 migration dry-run/schema drift/sqlx prepare | P1 |
| request_logs / usage 热表性能 | 存在 | `usage_records` 单宽表；Timescale 注释未启用；dashboard/filter options 多次扫表 group/distinct | P1 |
| 后台任务 worker 化和 lease | 存在 | health_check 有 advisory lock；health_probe / pricing sync / inflight sweep 无 lease，且都在 web server runtime 启动 | P0 |
| 大 handler/page 与 bounded context | 存在 | `admin.rs` 2147 行、`router.rs` 2003 行、`web/src/routes/channels/+page.svelte` 1235 行 | P1 |
| API contract / route manifest | 存在缺口 | 依赖手写 route + 手写 `web/src/lib/api.ts`；Cargo 已引 `utoipa` 但未见 OpenAPI 导出 | P2 |
| CI 性能/安全/DB 门禁 | 存在缺口 | CI 只有 fmt/clippy/check/test/web；无 cargo audit/deny、gitleaks、migration service、perf smoke、bundle budget | P1 |
| SLO / 业务观测 | 部分存在 | 有 Prometheus、health endpoints；缺 usage/outbox lag、pipeline stage latency、settle failure 指标 | P2 |

## 2. 当前证据地图

### 2.1 架构 / 启动 / 路由

- `crates/gate-server/src/main.rs:43-57`：启动时选择 InMemory 或 Pg repos，Pg 路径连接数据库并直接跑 `gate_storage::run_migrations`。
- `crates/gate-server/src/main.rs:59-141`：同一个 `AppState` 装配 Redis rate limiter、KMS、ProviderRouter、fallback provider，然后构建完整 router。
- `crates/gate-server/src/main.rs:143-190`：同一个 HTTP binary 内启动 `health_probe`、`health_check`、pricing sync、inflight sweep。
- `crates/gate-server/src/app.rs:19-33`：所有 `/v1` route 统一叠 `rls_inject`、`quota_enforce`、`rate_limit_by_subject`、metrics、trace、CORS。
- `crates/gate-server/src/routes/mod.rs:39-57`：`v1_router` 混合 `me/settings/models/embeddings/model_aliases/projects/api_keys/chat/images/audio/auth/sso/usage/channels/quotas/billing/admin/request_logs`。
- `crates/gate-server/src/state.rs:19-55`：`AppState` 同时承载 jwt、loader、repos、rate limiter、provider、provider_router、crypto、outbox、pricing、quota_counter、image/audio provider、audit。
- `crates/gate-server/src/state.rs:80-109`：`Repos` 聚合 users/orgs/projects/memberships/api_keys/channels/channel_keys/sso/usage/quotas/model_aliases/audit/billing/request_logs/inflight/pg_pool。

### 2.2 Provider runtime

- `crates/gate-providers/src/router.rs:1-15`：路由每次按 project/model 做 alias、查默认 group、查 healthy channel、构造 provider、取 key / env fallback。
- `crates/gate-providers/src/router.rs:738-775`：`route(project_id, requested_model)` 每请求执行 alias + 主模型路由 + fallback model chain。
- 未见 `RuntimeSnapshot` / atomic snapshot / mode-specific route manifest；`rg "RuntimeSnapshot|KOOIX_MODE|controlplane|data plane"` 无命中。

### 2.3 Gateway middleware / quota / billing

- `crates/gate-server/src/middleware/quota.rs:1-17`：quota middleware 定义 rpm/tpm/budget 执行语义，并明确多处 fail-open。
- `crates/gate-server/src/middleware/quota.rs:189-225`：middleware 先自行解析 auth，再从 repo 加载所有 scope quota；DB 失败 warn 并放行。
- `crates/gate-server/src/middleware/quota.rs:241-263`：有 budget quota 时读取 body 估算 cost；非 ChatRequest 走默认估值。
- `crates/gate-server/src/middleware/quota.rs:265-349`：逐条 pre-debit，生成 `InflightGuards`，并 best-effort 异步插入 `inflight_requests`。
- `crates/gate-server/src/routes/chat.rs:71-281`：chat handler 里同时做 provider resolve、params override、provider adapter、retry、metrics auto-disable、least_conn release、TPM record、quota settle、billing outbox emit。
- `crates/gate-server/src/billing_emit.rs:49-113`：billing emit 是 warn-only；未挂 outbox/pricing、查不到 pricing、enqueue 失败都不阻断。
- `crates/gate-server/src/billing_emit.rs:95-107`：UsageEvent 的 `request_id` 是 emit 时新生成，不是贯通 HTTP request_id / upstream id / inflight id。

### 2.4 Outbox / worker / 后台任务

- `crates/gate-billing/src/outbox.rs:57-67`：`fetch_batch` 只 `SELECT ... processed_at IS NULL ... LIMIT $1`，注释写明 C1 单消费者，不使用事务或 `FOR UPDATE SKIP LOCKED`。
- `crates/gate-billing/src/consumer.rs:41-48`：Consumer `run()` 是无限 loop + sleep，没有 context shutdown。
- `crates/gate-billing/src/consumer.rs:65-88`：单条处理失败 mark_failed，成功 mark_done；并发消费者下可能重复取同一批。
- `rg "Consumer::new|consumer.run" crates/gate-server crates/kgctl` 未发现生产启动路径；目前 consumer 主要在测试里使用。
- `crates/gate-server/src/health_check.rs:58-87`：health_check 有 `pg_try_advisory_lock`，这是好信号。
- `crates/gate-server/src/health_probe.rs:9-91`：health_probe 无 advisory lock；多实例会重复探活与更新 channel health。
- `crates/gate-server/src/main.rs:148-168`：pricing sync 每个 web 实例都会跑，无 lease。
- `crates/gate-server/src/main.rs:171-190`：inflight sweep 每个 web 实例都会跑；`DELETE ... RETURNING` 能降低重复 refund，但没有明确 worker owner / metric / shutdown。

### 2.5 数据库 / usage / migration

- `crates/gate-storage/src/lib.rs:53-61`：migration 由 `sqlx::migrate!("./migrations")` 内嵌，优于 FOXNIO 的 schema 常量 + 内联 migration 双轨。
- `crates/gate-storage/migrations/` 当前 25 个 SQL 文件，命名版本化；无明显“外部 migration 未接入启动链”问题。
- `crates/gate-storage/migrations/20260513000007_usage.sql:6-42`：`usage_records` 是宽时序表，包含归属、路由、token、cost、latency、error、client_ip、metadata。
- `20260513000007_usage.sql:51-59`：Timescale hypertable / compression / retention 只是注释，默认普通表。
- `crates/gate-storage/src/repo/request_log.rs:498-612`：dashboard stats 一次请求执行 total、top models、hourly trend、recent errors 等多次聚合/列表查询。
- `crates/gate-storage/src/repo/request_log.rs:615-701`：filter options 对 usage_records 做多个 DISTINCT 和 LEFT JOIN 查询。
- `crates/gate-storage/src/repo/usage.rs:118-165`：usage aggregate/totals 直接扫 `usage_records` GROUP BY/SUM，无 rollup/read model。
- `crates/gate-storage/src/repo/request_log.rs:350-352`：请求日志列表已用 keyset cursor，这是优点；但表仍宽且没有 detail 拆表。

### 2.6 代码质量 / 模块体量

按 `wc -l` 与 `checking-code-quality` 阈值（文件 <=500 行）看，当前超阈值集中在：

- `crates/gate-server/src/routes/admin.rs`：2147 行。
- `crates/gate-providers/src/router.rs`：2003 行。
- `crates/gate-storage/src/repo/channel.rs`：1345 行。
- `crates/gate-providers/src/custom_provider.rs`：1318 行。
- `web/src/lib/api.ts`：1300 行，质量脚本报告 1097 code lines > 500。
- `web/src/routes/channels/+page.svelte`：1235 行。
- `crates/gate-storage/src/repo/request_log.rs`：760 行。
- `crates/gate-billing/src/pricing.rs`：710 行。
- `web/src/routes/admin/groups/+page.svelte`：723 行。
- `web/src/routes/admin/requests/+page.svelte`：628 行。

### 2.7 CI / 发布门禁

- `.github/workflows/ci.yml:16-61`：CI 覆盖 `git diff --check`、`cargo fmt`、`cargo clippy -D warnings`、`cargo check`、`cargo test --workspace`、`npm run check/test/build`。
- 未见 CI 中显式 Postgres service + `kgctl migrate --dry-run` / 空库 migration dry-run / schema drift / `cargo sqlx prepare --check`。
- 未见 `cargo audit` / `cargo deny` / `gitleaks` / `semgrep` / `trivy` / frontend bundle budget。
- `Cargo.toml:97-99` 已引入 `utoipa`，但未见 OpenAPI spec 导出和 route manifest gate。

## 3. P0 TODO（先斩，不然后续功能会放大债务）

### P0-1. 拆出 Gateway / Control Plane / Worker runtime modes

- Problem：热路径、控制面、后台任务全部在 `gate-server` 一个 runtime 内装配，无法独立扩缩容、灰度、降级。
- Evidence：`main.rs:43-190`、`app.rs:19-33`、`routes/mod.rs:39-57`。
- Refactor action：
  1. 新增 `crates/gate-server/src/modes.rs` 或 `app/{gateway,controlplane,worker}`。
  2. 增加 `KOOIX_MODE=all|gateway|controlplane|worker`，默认 `all` 兼容现部署。
  3. `gateway` 只挂 `/v1/chat/completions`、`/v1/embeddings`、`/v1/images/*`、`/v1/audio/*`、`/v1/models`、`/health`、`/metrics`。
  4. `controlplane` 挂 auth/setup/me/settings/projects/api_keys/channels/quotas/billing/admin/request_logs/usage。
  5. `worker` 跑 outbox consumer、pricing sync、health check/probe、inflight sweeper。
- Acceptance test：
  - `KOOIX_MODE=gateway` 下 `/v1/chat/completions` route 存在，`/v1/admin/channels` 返回 404/disabled。
  - `KOOIX_MODE=controlplane` 下 `/v1/admin/channels` 可用，`/v1/chat/completions` 不挂载。
  - `KOOIX_MODE=worker` 不监听 HTTP 或只监听 worker health endpoint。
  - 增加 route manifest test，禁止新控制面 route 混进 gateway。
- Risk：前端当前依赖 `/v1/*` 统一前缀；拆 mode 时要避免破坏 all mode。

### P0-2. 固化 Gateway pipeline 与事件模型

- Problem：当前请求流的关键语义散在 middleware + chat handler：auth 解析、rate limit、quota pre-debit、provider route、retry、metrics、settle、billing emit、request log 没有一个可回放 pipeline。
- Evidence：`app.rs:15-22`、`middleware/quota.rs:189-349`、`routes/chat.rs:71-281`、`billing_emit.rs:49-113`。
- Refactor action：
  1. 定义 `gateway::Pipeline` 阶段：`resolve_identity -> admission -> route -> execute -> meter -> settle -> audit/log`。
  2. 定义 `GatewayEvent` / `MeteringEvent`：贯通 `http_request_id`、`provider_request_id`、`inflight_request_id`、org/project/api_key/channel/model、tokens、raw_cost、final_cost、status、error、idempotency_key。
  3. chat/images/audio/embeddings 共用 pipeline skeleton，避免各 handler 自己 spawn billing。
  4. fail-open / fail-closed 语义写成枚举配置，至少对 billing emit 和 quota 明确默认策略。
- Acceptance test：
  - provider 失败、stream 中断、无 usage frame、pricing miss、quota Redis down、outbox down 都有 golden tests。
  - 任一成功 API key 请求最多产生一个 metering event；重放同 idempotency_key 不重复写账。
- Risk：现阶段 billing 是 warn-only，切换到强一致会影响可用性；先以 shadow mode 记录事件再收紧。

### P0-3. 真正启用并加固 outbox consumer

- Problem：usage outbox 已实现，但生产 `gate-server` 未启动 consumer；即使启动，多实例 fetch_batch 没有 lock，可能重复消费。
- Evidence：`outbox.rs:57-67`、`consumer.rs:41-88`、`rg "Consumer::new|consumer.run" crates/gate-server crates/kgctl` 只在测试命中。
- Refactor action：
  1. 在 `worker` mode 启动 `gate_billing::Consumer`。
  2. `PgOutboxRepo::fetch_batch` 改事务：`SELECT ... FOR UPDATE SKIP LOCKED` + mark processing lease，避免并发重复。
  3. Consumer `run(ctx)` 支持 graceful shutdown，tick 暴露 metrics：lag、batch size、retry、failed、dead letter。
  4. outbox payload 增加 idempotency_key，`usage_records` 写入用稳定 request_id，而不是 emit 时另造。
- Acceptance test：
  - 两个 consumer 并发处理 100 条 outbox，最终 `usage_records` 正好 100 条，pending 为 0。
  - SIGTERM 后 worker 在 15 秒内退出。
  - `billing_outbox_lag_seconds` / `billing_outbox_failed_total` 可在 `/metrics` 看到。
- Risk：旧 pending outbox payload 没有新字段，需兼容 deserialize 或 migration backfill。

### P0-4. 后台任务 worker 化并清掉重复探活

- Problem：`health_check` 与 `health_probe` 功能重叠；一个有 advisory lock，一个没有。pricing sync 和 inflight sweep 也在 web runtime 中跑。
- Evidence：`main.rs:143-190`、`health_check.rs:58-87`、`health_probe.rs:9-91`。
- Refactor action：
  1. 合并 `health_probe` 到 `health_check`，保留有 auth/key、advisory lock、分级状态机的实现。
  2. pricing sync、inflight sweep 加 worker lease；或只在 `KOOIX_MODE=all|worker` 中由单 owner 运行。
  3. 所有 background loop 接收 cancellation token。
- Acceptance test：
  - 双实例下 health check/probe/pricing sync 只有一个 owner 执行。
  - 关闭实例不留下长时间任务；日志有 `worker stopped`。
- Risk：删除 `health_probe` 前确认 `health_check` 覆盖 disabled+unhealthy 恢复逻辑。

### P0-5. 贯通 request_id / idempotency_key

- Problem：HTTP request id、quota inflight request id、billing UsageEvent request_id 三者不一致，账务和日志难以对账。
- Evidence：`middleware/quota.rs:301` 生成 inflight request_id；`billing_emit.rs:95-107` 又生成 UsageEvent request_id；`audit_logs.request_id` 字段存在但不一定贯通。
- Refactor action：
  1. `base` middleware 生成一个 `KooixRequestId` extension。
  2. quota inflight、provider attempt、billing event、usage_records、audit_logs 都复用或派生该 id。
  3. streaming 场景将最终 usage 与同一个 idempotency_key 绑定。
- Acceptance test：
  - 一次 chat 请求能用同一个 request_id 查到 audit / usage / outbox / inflight lifecycle。
  - 重试同 idempotency_key 不重复 settle。
- Risk：外部 OpenAI-compatible 响应也有 id，命名要区分 `provider_request_id`。

## 4. P1 TODO（性能、数据库与工程门禁）

### P1-1. `usage_records` 拆 read model：事件窄表 + detail + rollup

- Problem：`usage_records` 单宽表承担日志列表、详情、dashboard、filter options、billing projection；规模上来后 dashboard 和筛选会扫热表。
- Evidence：`20260513000007_usage.sql:6-42`、`request_log.rs:498-701`、`usage.rs:118-165`。
- Refactor action：
  1. 新增 `request_events` 窄表：request_id、ts、org/project/api_key/channel/model/status/latency/cost/tokens。
  2. 新增 `request_event_details`：metadata、error payload、provider raw id、trace detail。
  3. 新增 `usage_hourly_rollups` / `usage_daily_rollups`。
  4. dashboard/filter/usage 优先读 rollup；日志列表只读窄表；详情按需查 detail。
- Acceptance test：
  - dashboard 常态 SQL 数 <= 3，且不扫 detail 表。
  - 100 万事件下 admin request list P95 < 500ms，dashboard P95 < 1s。
- Risk：历史数据迁移成本高；先双写新表和 rollup，再切读。

### P1-2. 把 Timescale/partition 从注释变成可选 migration path

- Problem：文档声称 5w rpm 级别建议 Timescale，但 migration 里 hypertable 只是注释；默认普通表没有自动分区/保留策略。
- Evidence：`20260513000007_usage.sql:51-59`、`DESIGN.md:230-233`。
- Refactor action：
  1. 增加可选 migration 或 `kgctl usage-storage upgrade --timescale`。
  2. 普通 PostgreSQL 路径提供月分区 + 自动未来分区创建 + retention job。
  3. CI 加普通 PG migration dry-run；nightly 加 Timescale profile。
- Acceptance test：
  - 普通 PG 空库 migrate 成功。
  - Timescale enabled 环境下 `usage_records` / 新事件表成功转 hypertable。
  - 分区/retention 任务有 dry-run 输出。
- Risk：不能把 Timescale 变成默认硬依赖，当前 README 明确普通 PG 可跑。

### P1-3. Provider runtime snapshot / route decision trace

- Problem：ProviderRouter 当前每请求查 repo、构造 provider、跑 fallback；控制面变更和热路径读取没有 snapshot version。
- Evidence：`router.rs:1-15`、`router.rs:738-775`。
- Refactor action：
  1. control plane 变更 channels/groups/keys/model_aliases 后编译 `ProviderRuntimeSnapshot`。
  2. gateway 只读 atomic snapshot，provider/key material 预构建或 lazy cache，并有 version。
  3. 每次 route 输出 decision trace：snapshot_version、alias、candidate channels、selected channel、fallback reason。
- Acceptance test：
  - 热更新后新请求看到新 snapshot version，旧请求保持旧决策。
  - Provider routing 单测不依赖 DB repo；可并行跑。
- Risk：key 解密缓存要处理轮转和 revoke，避免缓存旧密钥过久。

### P1-4. CI 增加 DB / 安全 / 性能门禁

- Problem：现 CI 是基本编译测试门禁，没有 DB migration service、schema drift、dependency/security scan、perf smoke。
- Evidence：`.github/workflows/ci.yml:16-61`。
- Refactor action：
  1. CI 增加 Postgres service，跑 `cargo run -p kgctl -- migrate --dry-run` 和空库 migrate。
  2. 若继续坚持编译期 SQL 校验，补 `cargo sqlx prepare --workspace --check` 或明确不用 `query!` 宏的 waiver。
  3. 加 `cargo audit` 或 `cargo deny`、gitleaks、Docker image scan。
  4. 加 lightweight perf smoke：mock provider 下 `/v1/models`、`/v1/chat/completions`、admin requests/dashboard。
  5. 前端加 bundle budget，避免 channels/admin 页面继续膨胀。
- Acceptance test：
  - PR 门禁 10-15 分钟内完成，失败信息指向具体 gate。
  - nightly heavy gate 可跑 Timescale/profile/load test。
- Risk：门禁太重会拖慢迭代；把 perf/load 放 nightly，PR 只 smoke。

### P1-5. 拆大文件和 bounded context

- Problem：多个核心文件远超 500 行，风险集中在 admin、provider router、channel repo、request log、frontend API/page。
- Evidence：见 2.6。
- Refactor action：
  1. `routes/admin.rs` 拆成 `admin/channels.rs`、`admin/channel_keys.rs`、`admin/orgs.rs`、`admin/users.rs`、`admin/groups.rs`、`admin/audit.rs`。
  2. `gate-providers/src/router.rs` 拆 `selection.rs`、`fallback.rs`、`key_resolver.rs`、`metrics.rs`、`snapshot.rs`。
  3. `repo/channel.rs` 拆 channel CRUD、groups、bindings、keys stats。
  4. `web/src/lib/api.ts` 按 domain 拆成 `api/channels.ts`、`api/admin.ts`、`api/usage.ts` 等，并由 barrel export。
  5. `web/src/routes/channels/+page.svelte` 拆 table/filter/modal/composables。
- Acceptance test：
  - 新增 quality gate：业务文件 <=500 行，超过需 `docs/waivers/quality/*.md` 说明。
  - `cargo clippy --workspace --all-targets -- -D warnings`、`npm run check` 通过。
- Risk：大规模 move 容易引入 import drift；先建立 facade，逐步搬。

## 5. P2 TODO（产品工程化与可观测性）

### P2-1. OpenAPI / route manifest / frontend typed client

- Problem：后端 route 和前端 API wrapper 都是手写，权限 metadata 也分散在 handler。
- Evidence：`Cargo.toml:97-99` 有 `utoipa` 依赖，但未见 OpenAPI 导出；`web/src/lib/api.ts` 1300 行。
- Refactor action：
  1. 为每条 route 标注 auth class：public/user/api_key/org_admin/platform_admin/internal。
  2. 导出 OpenAPI 或轻量 route manifest。
  3. frontend client 从 manifest 生成类型，禁止新增裸字符串 endpoint。
- Acceptance test：
  - CI 校验所有 route 有 auth class。
  - 前端 build 使用 generated types。

### P2-2. Ledger / billing projection 清晰化

- Problem：当前 `usage_records` 是 usage projection，不是不可变账本；billing emit warn-only，尚不能证明收入/成本完整性。
- Evidence：`billing_emit.rs:1-7`、`usage_records.cost_usd`、`ROADMAP.md` 已列 ledger 未完成。
- Refactor action：
  1. 新增 `billing_ledger_events(idempotency_key unique, direction, amount_micros, source_type, source_id, status, metadata)`。
  2. `usage_records` 退为 analytics projection。
  3. 对账任务从 ledger 重放生成 usage/billing summary。
- Acceptance test：
  - 任意 request_id 只 settle 一次。
  - 可从 ledger 重建 key spend / org monthly bill。

### P2-3. SLO 与业务指标

- Problem：已有 Prometheus HTTP metrics，但缺 pipeline stage、outbox lag、settle failure、snapshot version、job owner 等业务指标。
- Refactor action：
  - 增加 `gateway_stage_duration_seconds`、`provider_route_decisions_total`、`billing_outbox_lag_seconds`、`billing_settle_failures_total`、`usage_rollup_lag_seconds`、`worker_lease_owner`。
- Landed：指标已接入，`crates/gate-server/tests/perf_smoke.rs` 对 `/metrics` 做 smoke；查询样例见 `docs/observability-runbook.md`。
- Acceptance test：
  - `/metrics` smoke 能看到指标；Grafana dashboard 或 docs runbook 有查询样例。
- Risk：label cardinality 控制，api_key/user 不能直接做高基数 label。

## 6. 推荐实施顺序

### Week 1：先稳住 runtime 与账务事件

1. P0-3 启用 worker outbox consumer + SKIP LOCKED。
2. P0-5 贯通 request_id/idempotency_key。
3. P0-2 GatewayEvent/MeteringEvent shadow mode。
4. P0-4 合并 health_probe/health_check，worker lease skeleton。

### Week 2：拆模式但保持 all mode 兼容

1. P0-1 `KOOIX_MODE` 与 gateway/controlplane/worker router skeleton。
2. Route manifest test 确认 gateway/controlplane 分离。
3. Worker graceful shutdown + metrics。

### Week 3：usage 性能面

1. P1-1 新 request_events/detail/rollup 双写。
2. dashboard/filter/usage 改 rollup/read model。
3. P1-2 Timescale/partition 可选路径。

### Week 4：工程门禁与大文件止血

1. P1-4 CI 加 DB/security/perf smoke。
2. P1-5 先拆 `routes/admin.rs` 与 `web/src/lib/api.ts`。
3. P2-1 route manifest / OpenAPI 导出。

## 7. 统一验收门禁

当前已有：

```bash
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
cd web && npm run check && npm test && npm run build && npm run bundle:budget
node scripts/quality-gate.mjs
node scripts/check-route-manifest.mjs
node scripts/generate-route-types.mjs --check
node scripts/perf-smoke.mjs
cargo run -p kgctl -- migrate --dry-run
cargo run -q -p kgctl -- usage-storage plan
cargo run -q -p kgctl -- usage-storage plan --timescale
```

## 8. Completion audit checklist

| Objective requirement | Artifact / evidence | Status |
|---|---|---|
| 读取参考文档 | 已读取 foxnio 同日重构审计文档全文 | done |
| 检查当前项目是否有同类问题 | 第 1 节逐坑位判定，存在/不存在/弱风险均给证据 | done |
| 架构问题 | P0-1/P0-2/P0-4/P1-3 覆盖 runtime、pipeline、worker、snapshot | done |
| 性能问题 | P1-1/P1-2/P1-4 覆盖 usage 热表、rollup、partition、perf gate | done |
| 数据库/计费问题 | P0-3/P0-5/P2-2 覆盖 outbox、idempotency、ledger/projection | done |
| 质量/工程化问题 | P1-5/P2-1/P1-4 覆盖大文件、route manifest、CI 安全/DB 门禁 | done |
| 给出可执行 TODO | 第 3-6 节按 P0/P1/P2 + Week 顺序拆解 | done |
| 验收门禁 | 第 7 节列出当前已接入命令 | done |

## 9. Final completion audit — prompt to artifact mapping

本节是最终封口清单：每个 TODO 都必须能落到真实代码、测试或命令证据，不能只靠“测试绿”泛化判定。

| TODO | Artifact evidence | Test / command evidence | Closure |
|---|---|---|---|
| P0-1 Gateway / Control Plane / Worker runtime modes | `crates/gate-server/src/modes.rs` 定义 `RuntimeMode` 与 `KOOIX_MODE`；`app.rs` 拆 `build_gateway_router` / `build_controlplane_router`；`main.rs` 只在 `all|worker` 启 worker；`worker` mode 不建 HTTP router | `crates/gate-server/tests/runtime_modes.rs` 覆盖 gateway 不泄露 admin、controlplane 不挂 chat、worker 无 HTTP、manifest public；`node scripts/check-route-manifest.mjs` 验 82 route / 6 data-plane | done |
| P0-2 Gateway pipeline 与事件模型 | `crates/gate-server/src/gateway.rs` 定义 `GatewayStage` / `GatewayEvent` / `MeteringEvent` / `FailurePolicy`；`routes/{chat,embeddings,images,audio}.rs` 接入 `record_stage`；`metrics.rs` 导出 `gateway_stage_duration_seconds` | `crates/gate-server/tests/billing_e2e.rs` 覆盖成功请求只 emit 一个 usage event、stream final usage；`crates/gate-server/tests/perf_smoke.rs` smoke `/metrics` 必含 pipeline metrics | done |
| P0-3 outbox consumer 启用与并发加固 | `worker.rs` 启 `gate_billing::Consumer`；`outbox.rs` transaction + `FOR UPDATE SKIP LOCKED` + lease columns；`consumer.rs::run_until` 支持 `CancellationToken`、lag/batch/failure metrics；`UsageEvent.idempotency_key` 兼容旧 payload | `crates/gate-billing/tests/outbox_consumer.rs::concurrent_consumers_do_not_double_consume` 验 2 consumers / 100 events 不重复；`commit_usage_writes_read_models_rollups_and_ledger_once` 验幂等一次 | done |
| P0-4 background jobs worker 化 / 删除重复探活 | `crates/gate-server/src/health_probe.rs` 已删除；`health_check.rs` 保留 advisory-lock 探活并支持 shutdown；`worker.rs` 统一 outbox / pricing sync / inflight sweep / health_check，pricing 与 inflight 通过 `pg_try_advisory_lock` 单 owner | `rg "health_probe" crates Cargo.toml .github scripts web` 无运行时代码引用；`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo test --workspace` 通过 | done |
| P0-5 request_id / idempotency 贯通 | `middleware/base.rs` 注入 `KooixRequestId` 并接受 `x-request-id`；`middleware/quota.rs`、`billing_emit.rs`、`consumer.rs` 复用 request_id/idempotency_key；`usage_records` / `request_events` / `billing_ledger_events` 均以稳定 key 去重 | `crates/gate-server/tests/quota_predebit.rs::request_id_is_shared_by_quota_inflight_and_billing_outbox`；`billing_e2e.rs::non_stream_apikey_emits_one_usage_event` | done |
| P1-1 usage read model：窄表 + detail + rollup | migration `20260519000002_request_events_rollups_ledger.sql` 新增 `request_events` / `request_event_details` / `usage_hourly_rollups` / `usage_daily_rollups`；`consumer.rs::commit_usage` 双写；`repo/request_log.rs` 与 `repo/usage.rs` 优先读新 read model / rollups，旧 `usage_records` fallback | `commit_usage_writes_read_models_rollups_and_ledger_once`；真实 Postgres `kgctl migrate --dry-run -> migrate -> dry-run` 已验证 pending 清零 | done |
| P1-2 Timescale / partition 可选路径 | `crates/kgctl/src/usage_storage.rs` 与 `main.rs` 增 `kgctl usage-storage plan [--partition|--timescale]`，只输出 dry-run DDL，不把 Timescale 变默认依赖；`crates/kgctl/README.md` 记录用法 | `cargo run -q -p kgctl -- usage-storage plan --partition` 输出 `request_events_partitioned`；`cargo run -q -p kgctl -- usage-storage plan --timescale` 输出 `create_hypertable` | done |
| P1-3 Provider runtime snapshot / route decision trace | `gate-providers/src/router.rs` 增 `RouteDecisionTrace` / candidate+skip trace / `ProviderRuntimeSnapshot` / versioned replace；`RoutedProvider.decision_trace` 暴露决策；`routes/chat.rs` 将 `resolved_model` 写回 upstream request | `route_decision_trace_records_candidates_and_selection`、`route_decision_trace_records_snapshot_version_and_alias`、`provider_runtime_snapshot_is_replaceable_and_versioned`、`crates/gate-server/tests/c1_routing.rs::full_chain_rewrites_model_from_alias_and_channel_mapping` | done |
| P1-4 CI DB / security / perf / bundle gates | `.github/workflows/ci.yml` 加 `quality-gate`、route manifest/type check、perf smoke、Postgres `17-alpine` migration gate、`cargo audit --ignore RUSTSEC-2023-0071`、`gitleaks`、web check/test/build/bundle budget | 本地已跑 `git diff --check`、`cargo fmt --all -- --check`、`cargo check --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cd web && npm run check && npm test && npm run build && npm run bundle:budget`；本机已安装并复验 `cargo-audit`；CI 对无上游修复的 `RUSTSEC-2023-0071` 使用精确 ignore，理由见 `docs/waivers/security/2026-05-19-rsa-marvin-openidconnect.md`；`gitleaks` 由 CI 执行 | done |
| P1-5 Large-file quality / bounded context guard | `scripts/quality-gate.mjs` 调用 `checking-code-quality`，新增未登记大文件直接 fail；`docs/waivers/quality/2026-05-19-large-files.md` 登记 legacy offenders 与 exit plan，避免本轮账务/runtime 大改混入高风险 move | `node scripts/quality-gate.mjs` 输出 legacy offenders 为 waived 且 `quality gate ok`；后续拆分必须逐项移出 waiver | accepted with waiver |
| P2-1 route manifest / frontend typed client seed | `crates/gate-server/src/route_manifest.rs` 为每 route 标注 auth class + mode；`GET /route-manifest.json` public export；`scripts/generate-route-types.mjs` 生成 `web/src/lib/api/route-manifest.ts` | `node scripts/check-route-manifest.mjs`：82 manifest routes / 82 served handlers / 6 data-plane；`node scripts/generate-route-types.mjs --check`：82 routes | done |
| P2-2 Ledger / billing projection | migration `20260519000002_request_events_rollups_ledger.sql` 新增 `billing_ledger_events(idempotency_key unique, direction, amount_micros, source_type, source_id, status, metadata)`；`usage_records` 继续作为 analytics projection；`commit_usage` 同步 ledger | `commit_usage_writes_read_models_rollups_and_ledger_once` 验重复 settle 不重复写 `usage_records` / `request_events` / rollups / ledger | done |
| P2-3 SLO metrics / runbook / smoke | `metrics.rs` 增 `gateway_stage_duration_seconds`、`provider_route_decisions_total`、`billing_outbox_lag_seconds`、`billing_settle_failures_total`、`usage_rollup_lag_seconds`、`worker_lease_owner`；`docs/observability-runbook.md` 给 PromQL | `crates/gate-server/tests/perf_smoke.rs` 覆盖 `/v1/models`、`/v1/chat/completions`、`/v1/admin/dashboard-stats`、`/metrics`；`node scripts/perf-smoke.mjs` 通过 | done |

Final smoke after this mapping:

```bash
git diff --check
cargo fmt --all -- --check
node scripts/check-route-manifest.mjs
node scripts/generate-route-types.mjs --check
node scripts/quality-gate.mjs
node scripts/perf-smoke.mjs --check-script-only
rg -n "<stale-status-marker-regex>" docs/stages/2026-05-19-refactor-todo-audit.md
```

Result：route manifest / generated types / quality gate / perf smoke script 全部通过；文档无未收口残留标记。
