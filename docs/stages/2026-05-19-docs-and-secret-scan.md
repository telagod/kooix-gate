# 文档分层与 Secret Scan 收口

Status: applied
Scope: 文档入口清理、阶段性文档归档、gitleaks 本地安装复验、HTTP Plugin secret slots 与 P1.8 Plugin Ecosystem 收口。
Last verified: 2026-05-20

## 关键文档 vs 阶段性文档

- 关键入口保留在根目录与 `docs/README.md` 索引：`README.md`、`DESIGN.md`、`ROADMAP.md`、`CHANGELOG.md`、`RELEASE.md`、`AGENTS.md`、`CLAUDE.md`、`docs/plugin-manifest.md`、`docs/security-runbook.md`、`docs/observability-runbook.md`。
- 模块文档保留在对应模块：`web/README.md`、`web/src/lib/design/README.md`、`crates/kgctl/README.md`、`bench/README.md`、`examples/README.md`。
- 已完成的一次性审计、迁移、收口和验证快照统一放入 `docs/stages/`，不再散落根目录。
- active waiver 仍放在 `docs/waivers/`，因为脚本 / CI 可能引用，暂不归档到 stages。

## gitleaks

- 本机安装位置：`/home/telagod/.local/bin/gitleaks`
- 版本：`8.30.1`
- CI：`.github/workflows/ci.yml` 的 `Security Smoke` job 已使用 `gitleaks/gitleaks-action@v2`。

本地验收命令：

```bash
gitleaks version
gitleaks detect --source . --redact --verbose
tmp=$(mktemp -d) && git ls-files -co --exclude-standard -z | tar --null -T - -cf - | tar -C "$tmp" -xf - && gitleaks detect --source "$tmp" --no-git --redact --verbose
```

## P1.9 Observability / Trace correlation

本轮把 P1.9 Trace 串联从路线项落成 data-plane + billing 可追踪闭环：

- HTTP middleware 使用固定 `http.request` span，记录 `request_id`、`status` 与 `latency_ms`，并把 `x-request-id` 写入 OTEL attribute `kooix.request_id`。
- 新增 `trace_context` helper，统一生成 `gateway.data_plane` 与 `gateway.upstream_request` spans，并携带 `request_id`、`org_id`、`project_id`、`api_key_id`、`user_id`、`channel_id`、`group_id`、`provider_type`、`endpoint`、`model`。
- `chat` / `responses` / `embeddings` / `images` / `audio` 所有 upstream provider call 都有 `gateway.upstream_request` child span，记录 `operation`、`streaming`、`outcome` 与 `duration_ms`；retry 场景每次 attempt 单独生成 span。
- data-plane 中所有 `emit_usage` spawned task 都继承对应 `gateway.data_plane` span，避免成功返回后 billing trace 断链。
- `billing.emit_usage` 记录 pricing / enqueue outcome；`billing.outbox.enqueue|fetch_batch|mark_done|mark_failed` 与 `billing.consumer.tick|process_one|commit_usage` 覆盖 outbox 生命周期和 settlement 落库。
- `docs/observability-runbook.md` 增加 trace span / attribute 清单与按 request_id 排障顺序。

验证命令：

```bash
cargo fmt --all -- --check
cargo check -p gate-billing -p gate-server --all-targets
cargo clippy -p gate-billing -p gate-server --all-targets -- -D warnings
cargo test -p gate-server trace_context
cargo test -p gate-billing --test outbox_consumer -- --nocapture
```

## P1.9 Observability / Incident center

本轮把 P1.9 控制台事故页落成可操作的止血入口：

- 新增 `GET /v1/admin/incidents?org_id=<uuid>&hours=24`，权限沿用 Platform Admin + `AuditRead`。
- 后端从 `request_events`（缺表时回退 `usage_records`）汇总最近错误、top failing channels 与 upstream 401 / 429 / 5xx / other / unknown 分类。
- `metrics.rs` 为 `quota_denies_total` 与 `gateway_upstream_errors_total` 同步维护 bounded process-local snapshots，事故页展示 quota deny top 与运行时上游错误 Top；Prometheus 仍是跨实例长期趋势来源。
- 控制台新增 `/admin/incidents`，按中文 UI 展示最近错误、失败渠道、quota deny top、upstream 分类与运行时快照，并从 Sidebar / Dashboard 提供入口。
- `docs/observability-runbook.md` 增加 Console incident center 判读顺序和 runtime-local caveat。

验证命令：

```bash
cargo fmt --all -- --check
cargo check -p gate-server --all-targets
cargo test -p gate-server admin_incidents_requires_platform_admin_user -- --nocapture
cargo test -p gate-server runtime_snapshots_capture_bounded_metrics -- --nocapture
npm --prefix web run check
npm --prefix web test
node scripts/check-route-manifest.mjs
node scripts/generate-route-types.mjs --check
```

## P1.9 Observability / Operations runbook

本轮把 P1.9 Runbook 从路线项收口为长期运维文档，仍保持文档分层干净：

- 关键文档落在 `docs/observability-runbook.md` 的 `Incident runbooks`，覆盖上游全挂、Redis 不可用、Postgres 慢查询、pricing sync 失败与 outbox backlog。
- 每个事故条目固定 `Signals -> 止血 -> 诊断 -> 恢复 / 验证`，同时给出 PromQL、SQL、`kgctl`、`curl` 与 Redis / Postgres 命令。
- `ROADMAP.md` 将 P1.9 Runbook 子项全部勾选；`CHANGELOG.md` 在 Unreleased 记录该运维收口。
- 文档分层继续遵守本文件开头规则：长期运维规则只留在关键 runbook，阶段性证据追加在 `docs/stages/`，根目录不新增完成态散文档。

阶段验证命令：

```bash
git diff --check
rg -n "上游全挂|Redis 不可用|Postgres 慢查询|pricing sync 失败|outbox backlog|Incident runbooks" docs/observability-runbook.md ROADMAP.md CHANGELOG.md
```

## P2.1 Frontend UX / template consistency audit

本轮先把 P2.1 “全页面套模板一致性审计”做成可重复执行的证据，而不是只靠人工印象：

- 新增 `scripts/audit-page-templates.mjs`，扫描全部 `web/src/routes/**/+page.svelte`。
- 审计维度覆盖 P2.1 子项：header shell、toolbar、filter、table、empty / loading / error 状态。
- 脚本区分公共首页 / 登录 / 初始化 / 邀请接受 / Playground 这类例外 shell，避免把全屏画布或公开页误判为控制台页。
- 当前快照：25 个 route page，13 个仍有模板化缺口；缺口集中在旧控制台页的 `PageShell` / `DataTable` / `DataToolbar` 迁移。
- `web/README.md` 记录审计命令；`ROADMAP.md` 将审计子项勾选，后续 P2.1 继续按该清单逐页迁移表格能力与 wizard。

阶段验证命令：

```bash
node scripts/audit-page-templates.mjs
node scripts/audit-page-templates.mjs --json
node scripts/audit-page-templates.mjs --fail-on-gaps
```

`--fail-on-gaps` 当前预期非零，用于后续缺口清零后接入 CI。

## P2.1 Frontend UX / table capability base

本轮推进 P2.1 “表格能力统一”的第一块基座，并先迁一个旧页验证闭环：

- 新增 `web/src/lib/table-state.ts`，统一 page size / offset、`sort_by` / `sort_dir`、column visibility、saved filters 的规范化与 localStorage 持久化。
- `/admin/audit` 从手写 header / toolbar / native table 迁到 `PageShell`、`DataToolbar`、`DataTable`、`StatePanel`，模板审计缺口从 13 页降到 12 页。
- `/admin/audit` 支持 page size、上一页/下一页 offset 分页、表头排序、列显隐与持久化筛选状态；required columns 不允许隐藏。
- 后端 `GET /v1/admin/audit-logs` 增加枚举化 `sort_by` / `sort_dir`，仅允许 `ts`、`actor_kind`、`action`、`resource_kind`、`outcome`，避免任意 SQL 拼接。
- `gate-storage` 增加 `AuditSortBy` / `SortDirection` 与 `list_by_org_sorted`，Pg 查询使用白名单列名，InMemory 实现补稳定排序与分页测试。
- `ROADMAP.md` 将表格能力四个子项标记为基座完成，保留“推广到剩余数据页”作为未完成项。

阶段验证命令：

```bash
cargo fmt --all
cargo test -p gate-storage repo::audit -- --nocapture
cargo test -p gate-server admin_audit_logs_support_pagination_and_sort_query -- --nocapture
npm --prefix web run check
npm --prefix web test -- table-state.test.ts
node scripts/audit-page-templates.mjs
```

## P2.1 Frontend UX / admin users table rollout

本轮继续按 P2.1 “表格能力统一”推广到 `/admin/users`，不改后端查询契约：

- `/admin/users` 用户列表从手写 toolbar / native table 迁到 `DataToolbar` 与 `DataTable`。
- 列表状态接入 `web/src/lib/table-state.ts`：page size、column visibility、搜索词、状态筛选统一 localStorage 持久化。
- 用户列表补齐列显隐控制，`email` / `status` / `actions` 作为 required columns 不允许隐藏。
- reset password 与 session 面板改用 `ModalFrame`；session 内表格同步改用 `DataTable`，避免页面继续保留 native table。
- 模板审计快照：25 个 route page，`/admin/users` gaps 清零，pages_with_gaps 从 12 降到 11。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`web/README.md`、`web/src/lib/design/README.md`。

阶段验证命令：

```bash
npm --prefix web run check
npm --prefix web test -- table-state.test.ts
node scripts/audit-page-templates.mjs
```

## P2.1 Frontend UX / admin incidents table rollout

本轮继续按 P2.1 “表格能力统一”推广到 `/admin/incidents`：

- 事故中心 `Top failing channels` 从 native table 迁到共享 `DataTable`，保留错误数比例条、错误率和最近错误信息。
- `/admin/incidents` 不引入额外后端查询参数，只收敛展示层模板，避免把事故摘要页误改成分页列表页。
- 模板审计快照：25 个 route page，`/admin/incidents` gaps 清零，pages_with_gaps 从 11 降到 10。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`web/README.md`、`web/src/lib/design/README.md`。

阶段验证命令：

```bash
npm --prefix web run check
node scripts/audit-page-templates.mjs
```

## P2.1 Frontend UX / org quotas table rollout

本轮继续按 P2.1 “表格能力统一”推广到 `/orgs/[orgId]/quotas`：

- Quota policy 列表新增 `DataToolbar`，支持按维度 / scope / model / quota ID 搜索，并按 scope、mode 做 quick filters。
- 分组 quota 表格从 native table 迁到共享 `DataTable`，保留 scope 分组、dimension/unit、mode badge、Explain 与 delete 操作。
- 空态细分为“暂无配额规则”和“无匹配配额规则”，筛选空态提供清除筛选动作。
- 模板审计快照：25 个 route page，`/orgs/[orgId]/quotas` gaps 清零，pages_with_gaps 从 10 降到 9。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`web/README.md`、`web/src/lib/design/README.md`。

阶段验证命令：

```bash
npm --prefix web run check
node scripts/audit-page-templates.mjs
```

## P2.1 Frontend UX / org billing table rollout

本轮继续按 P2.1 “表格能力统一”推广到 `/orgs/[orgId]/billing`：

- 月账单页从手写 breadcrumb / header 迁到共享 `PageShell`，标题、说明、组织短 ID 与 icon 节奏统一。
- 月份选择、刷新、CSV / JSON 导出与 digest 展示收敛到 `DataToolbar`，保留原有导出文件名、JSON digest 与 invoice exported 前置校验。
- Project / Model 两组 breakdown 从 native table 迁到共享 `DataTable`，空态走模板 `empty` snippet；loading / error 改用 `StatePanel`。
- Quota alerts 保留 watch / approaching / exceeded 三段语义，只收敛到共享 `Card` / `Badge` / `text` token。
- 模板审计快照：25 个 route page，`/orgs/[orgId]/billing` gaps 清零，pages_with_gaps 从 9 降到 8。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`web/README.md`、`web/src/lib/design/README.md`。

阶段验证命令：

```bash
npm --prefix web run check
node scripts/audit-page-templates.mjs
```

## P2.1 Frontend UX / org projects table rollout

本轮继续按 P2.1 “表格能力统一”推广到 `/orgs/[orgId]/projects`：

- Project 列表页从手写 breadcrumb / H1 迁到共享 `PageShell`，保留账单、配额管理与创建项目三个入口动作。
- 新建 Project 表单改用 `Card` / `Field` / `Alert` / icon button 节奏，继续保留 name + slug 必填和后端错误回显。
- Project 列表从 native table 迁到共享 `DataTable`，状态显示改用 `Badge`，空态、loading、error 改用 `StatePanel` / template empty snippet。
- Org invite 面板继续复用 `InvitationPanel`，不改变邀请 token 创建、复制、撤销链路。
- 模板审计快照：25 个 route page，`/orgs/[orgId]/projects` gaps 清零，pages_with_gaps 从 8 降到 7。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`web/README.md`、`web/src/lib/design/README.md`。

阶段验证命令：

```bash
npm --prefix web run check
node scripts/audit-page-templates.mjs
```

## P2.1 Frontend UX / project keys table rollout

本轮继续按 P2.1 “表格能力统一”推广到 `/orgs/[orgId]/projects/[projectId]/keys`：

- API Key 管理页从手写 breadcrumb / header 迁到共享 `PageShell`，保留返回项目设置、刷新、创建 Key 三个动作。
- 创建 Key 表单改用 `Card` / `Field` / `Alert`，继续保留名称必填、后端错误回显与创建后列表刷新。
- 明文 Key 一次性展示改用 success `Card` 与 copy action，保留关闭后 toast 提醒，避免改变 secret exposure 语义。
- Key 列表从 native table 迁到共享 `DataTable`，状态显示改用 `Badge`，loading / error / empty 改用 `StatePanel` / template empty snippet。
- 撤销确认从手写 fixed overlay 迁到共享 `ModalFrame`，保留 destructive confirm 和撤销后列表刷新。
- 模板审计快照：25 个 route page，`/orgs/[orgId]/projects/[projectId]/keys` gaps 清零，pages_with_gaps 从 7 降到 6。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`web/README.md`、`web/src/lib/design/README.md`。

阶段验证命令：

```bash
npm --prefix web run check
node scripts/audit-page-templates.mjs
```

## P2.1 Frontend UX / project detail shell rollout

本轮继续按 P2.1 “表格能力统一”清理 `/orgs/[orgId]/projects/[projectId]` 的最后一个 `/orgs/*` 模板缺口：

- Project 设置页从手写 H1 / header 迁到共享 `PageShell`，标题、项目 slug、组织短 ID 与 Project 短 ID 统一由模板呈现。
- loading / error 状态改用 `StatePanel`，保留 Project 设置、Project invite、API Key quick create 与模型别名增删链路。
- 页面新增模板 actions：返回 Project 列表与跳转专用 API Keys 页；URL 继续通过 `rawId()` 使用后端接受的裸 UUID。
- 模板审计快照：25 个 route page，`/orgs/[orgId]/projects/[projectId]` gaps 清零，pages_with_gaps 从 6 降到 5。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`web/README.md`、`web/src/lib/design/README.md`。

阶段验证命令：

```bash
npm --prefix web run check
node scripts/audit-page-templates.mjs
```

## P2.1 Frontend UX / admin SSO toolbar rollout

本轮继续按 P2.1 “表格能力统一”清理 `/admin/sso` 的工具栏模板缺口：

- SSO Provider 搜索区从手写 search card 迁到共享 `DataToolbar`，继续保留 name / slug / issuer / domain 搜索语义。
- 工具栏新增清除搜索动作，避免搜索词藏在输入框里导致误判空态。
- 工具栏 badges 展示当前匹配 provider 数与 enabled provider 数，和页面顶部三张统计卡形成轻量补充。
- 原有 OIDC discovery、allowlist、claim mapping、JIT auto-create、auto-join role、enabled 切换与 redirect policy 编辑链路不变。
- 模板审计快照：25 个 route page，`/admin/sso` gaps 清零，pages_with_gaps 从 5 降到 4。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`web/README.md`、`web/src/lib/design/README.md`。

阶段验证命令：

```bash
npm --prefix web run check
node scripts/audit-page-templates.mjs
```

## P2.1 Frontend UX / usage dashboard shell rollout

本轮继续按 P2.1 “表格能力统一”清理 `/usage` 的页面模板缺口：

- 用量仪表盘从手写 header / icon / H1 迁到共享 `PageShell`，页面说明统一展示当前 Org 或全平台视角。
- range 与 group_by 控件迁到共享 `DataToolbar`，并用 badges 展示当前趋势维度与时间范围。
- error 状态改用 `StatePanel`；loading skeleton、stat cards、chart mode 切换、每日趋势折线图、模型/渠道横向柱状图与 Org/date range footnote 保持原行为。
- 模板审计快照：25 个 route page，`/usage` gaps 清零，pages_with_gaps 从 4 降到 3。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`web/README.md`、`web/src/lib/design/README.md`。

阶段验证命令：

```bash
npm --prefix web run check
node scripts/audit-page-templates.mjs
```

## P2.1 Frontend UX / setup auth frame rollout

本轮继续按 P2.1 “表格能力统一”清理 `/setup` 的认证页模板缺口：

- 首次初始化页从手写 full-screen shell / Card 迁到共享 `AuthFrame`，和 `/login`、`/invite/accept` 使用同一无 sidebar 页面节奏。
- theme toggle 改用 `authTemplate.themeToggle`，避免 route 内复制固定定位按钮长 class。
- 保留两步 bootstrap：管理员邮箱/密码校验、默认 Org/Project 创建、初始化完成后用新管理员账号自动登录并跳转 `/orgs`。
- 模板审计快照：25 个 route page，`/setup` gaps 清零，pages_with_gaps 从 3 降到 2。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`web/README.md`、`web/src/lib/design/README.md`。

阶段验证命令：

```bash
npm --prefix web run check
node scripts/audit-page-templates.mjs
```

## P2.1 Frontend UX / admin groups template rollout

本轮继续按 P2.1 “表格能力统一”清理 `/admin/groups` 的控制台模板缺口：

- 渠道分组页从手写 header / H1 迁到共享 `PageShell`，页面说明集中展示 group/fallback/canary 职责。
- 顶层加载失败与空状态改用 `StatePanel`，保留分组卡片、启停 switch、fallback chain、编辑表单、删除确认与添加渠道 modal 行为。
- Canary 对比表与绑定渠道列表从 native table 迁到共享 `DataTable`，统一表头、边框、hover 与空态审计信号。
- 模板审计快照：25 个 route page，`/admin/groups` gaps 清零，pages_with_gaps 从 2 降到 1。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`web/README.md`、`web/src/lib/design/README.md`。

阶段验证命令：

```bash
npm --prefix web run check
node scripts/audit-page-templates.mjs
```

## P1.5 Billing ledger / reconciliation / invoice state / export digest

本轮把 P1.5 billing 全部推进成可对账闭环：

- `billing_ledger_events` 新增显式 `event_type`：`estimated_debit` / `actual_settle` / `refund` / `manual_adjustment` / `invoice_close`，并补 `invoice_month` 与 org-level adjustment / invoice close 可空 project/api_key。
- `gate-billing::ledger` 提供 typed constructors 与幂等 `insert_ledger_event`；`commit_usage` 默认写 `actual_settle`，不再只靠 `direction=debit` 表达语义。
- `gate-billing::reconciliation::reconcile_usage_ledger` 按窗口对比 `usage_records` 与 posted `actual_settle` ledger，输出 missing ledger / orphan ledger / amount mismatch。
- `PgBillingRepo::monthly_bill` 费用优先从 ledger 重建，tokens/model/project 仍读 `usage_records` analytics projection。
- `billing_invoices` 新增月账单状态机：`draft -> closed -> exported -> paid/waived`；`exported` 要求 `sha256:<hex>` digest，状态推进写 audit。
- Billing CSV 导出增加 `x-kooix-export-digest=sha256:<hex>`；新增 JSON 导出 `/v1/orgs/:org_id/billing/export.json`，响应内嵌 `digest`。
- Pricing 控制台新增 Conditions JSON editor 与 cache / image size / audio seconds / batch / region 模板。
- 成本告警扩展预算 50/80/100% 阈值，并保留 pricing miss 与高成本异常观测入口；具体判读写入 `docs/observability-runbook.md`。

验证命令：

```bash
cargo fmt --all -- --check
cargo test -p gate-billing --test outbox_consumer ledger_event_model_accepts_p1_5_event_types -- --nocapture
cargo test -p gate-billing --test outbox_consumer reconcile_usage_records_with_actual_settle_ledger -- --nocapture
cargo test -p gate-storage --test pg_repo billing_invoice_state_machine_persists_forward_transitions -- --nocapture
cargo test -p gate-server routes::billing::tests -- --nocapture
cargo test -p gate-server alerts::tests -- --nocapture
npm --prefix web run check
```

## P1.4 Canary routing

本轮把 P1.4 最后一项 Canary routing 落成小流量 + 自动对比闭环：

- `channel_group_bindings` 新增 `canary_percent_bps`，`NULL` 表示普通 binding，`100..500` 表示 1%-5% canary 流量；migration 保留 DB 级 `0..10000` 约束以便后续扩展。
- `ChannelBinding` / `ChannelGroupRepo::add_binding` / `update_binding` / `list_bindings` / `list_healthy_in_group` 全链路读写 canary 字段；内存 repo 也补齐 admin binding 行为，避免 dev/test 控制面空读。
- `ProviderRouter` 增加 deterministic canary gate：未命中的 canary binding 记录 `canary_not_selected` skip trace，并继续让普通 binding 接流量；不再用 `weight` 冒充灰度百分比。
- Admin API 校验 canary 只能配置为 1%-5% 或 `null` 关闭；Group detail 新增 `canary_stats`，按近 24h `request_events` 比较请求数、错误率、平均延迟、平均成本。
- 控制台 `/admin/groups` 支持添加 / 编辑 binding 时设置 Canary 百分比，并新增 “Canary 对比” 面板，展示 canary 相对 baseline 的错误率 / 延迟 / 成本差值。
- `docs/observability-runbook.md` 增加 Canary routing 运维 SQL 与判读规则。

验证命令：

```bash
cargo fmt --all -- --check
cargo check -p gate-storage -p gate-providers -p gate-server --all-targets
cargo test -p gate-storage --test channel_repo channel_binding_canary_percent_roundtrips -- --nocapture
cargo test -p gate-providers canary -- --nocapture
cargo test -p gate-server --test auth_flow admin_group_binding_canary_validates_and_returns_stats_shape -- --nocapture
npm --prefix web run check
```

## P1.4 Channel draining

本轮把 P1.4 的 Channel draining 从路线项落成可操作的运维闭环：

- `channels.status` 新增 `draining`，migration 保持旧 `active|disabled|deleted` 兼容并扩展 check constraint。
- `ChannelRepo::set_status` 统一 control-plane 状态切换；`ChannelRecord::is_healthy()` 仍只接受 `active + healthy`，因此 `draining + healthy` 会自然被新请求路由跳过。
- Admin API 增加：
  - `POST /v1/admin/channels/:id/drain`：进入 draining，返回 channel + 当前 inflight。
  - `GET /v1/admin/channels/:id/drain-status`：刷新当前 inflight 与 `safe_to_disable`。
  - `POST /v1/admin/channels/:id/disable-when-idle`：仅在 inflight 清空后改为 disabled，否则返回 400。
- `ProviderRouter::InflightTracker` 作为本阶段 draining 的等待依据；least_conn 请求生命周期已有 acquire/release，非 least_conn 当前返回 0。
- Channel 列表与详情页新增 Drain、空闲禁用、inflight 刷新与 Draining badge；`/admin/channels` 仪表盘新增 Draining 统计。
- API subject 仍强制 `require_user!`，API key 不能调用管理 drain endpoint。

验证命令：

```bash
cargo fmt --all -- --check
cargo check -p gate-storage -p gate-providers -p gate-server --all-targets
cargo test -p gate-storage --test channel_repo draining_channel_is_persisted_and_excluded_from_healthy_group -- --nocapture
cargo test -p gate-providers route_skips_draining_channel -- --nocapture
cargo test -p gate-server --test auth_flow admin_channel_draining_stops_new_requests_and_waits_for_inflight -- --nocapture
cargo test -p gate-server --test auth_flow admin_channel_drain_rejects_api_key_subject -- --nocapture
npm --prefix web run check
npm --prefix web test -- api
```

## P1.4 `least_latency` 持久化滑窗

本轮把 `least_latency` 从单进程 `ChannelMetrics` 升级为可跨实例复用的持久化滑窗：

- 新增 migration `20260520000001_channel_latency_samples.sql` 与 `ChannelLatencyRepo`，记录 `channel_id`、`latency_ms`、`success`、`source=request|health_probe`。
- `ProviderRouter` 注入 `channel_latency_repo` 后，在 `least_latency` 策略中按候选 channel 一次批量查询近窗口成功均值；查询失败或无样本时回退内存 metrics，不阻断数据面。
- `chat`、`responses` 和后台 health probe 都写入 latency samples；流式请求以 stream 建立耗时作为首包/建立延迟样本。
- `docs/observability-runbook.md` 增加 DB 滑窗与 Prometheus probe 指标的职责边界：Prometheus 做趋势告警，DB 滑窗做路由决策。

验证命令：

```bash
cargo fmt --all -- --check
cargo check -p gate-storage -p gate-providers -p gate-server --all-targets
cargo test -p gate-storage --test channel_latency -- --nocapture
cargo test -p gate-providers least_latency -- --nocapture
cargo test -p gate-server --test c1_routing health_checker -- --nocapture
cargo test -p gate-server --all-targets
cargo test -p gate-providers --all-targets
cargo clippy --all-targets -- -D warnings
npm --prefix web run check
npm --prefix web test
/home/telagod/.local/bin/gitleaks detect --source . --redact --verbose
tmp=$(mktemp -d) && git ls-files -co --exclude-standard -z | tar --null -T - -cf - | tar -C "$tmp" -xf - && /home/telagod/.local/bin/gitleaks detect --source "$tmp" --no-git --redact --verbose
```

## P1.4 fallback 策略可视化

本轮把 fallback 从“配置字段存在”推进到控制面可解释、可验尸：

- Admin Group detail API 返回 `fallback_chain` 与 `fallback_stats`，包含链路节点、节点 channel 数、近 24h `request_events.group_id` 请求量、节点占比、primary/fallback 请求量与 fallback hit-rate。
- Admin Group create/update 接收并持久化 `description` / `fallback_group_id`；fallback 更新前校验目标存在、禁止自引用、禁止 A→B→A 等循环，并限制最大深度 5。
- billing usage event 增加可选 `group_id`；chat、responses、embeddings、images、audio 路由命中后会把 group_id 写入 outbox，consumer 双写到 `request_events.group_id` 与 `usage_records.group_id`。
- 控制台 `/admin/groups` 展示 fallback chain 图、节点请求占比、primary/fallback counters、fallback hit-rate 与 cycle warning；编辑候选会过滤掉会形成环的分组。
- 前端 `GroupDetail` 同时兼容旧 `project_ids` 与后端当前 `projects_using`，避免删除提醒与 detail 面板继续漂移。

验证命令：

```bash
cargo fmt --all -- --check
cargo check -p gate-billing -p gate-providers -p gate-server --all-targets
cargo test -p gate-server --test auth_flow admin_group_detail_exposes_fallback_chain_and_validates_cycles -- --nocapture
npm --prefix web run check
```

## P1.4 health probe standardization

本轮收口 P1.4 首项 Health probe 标准化：

- Compile-time provider 不再只有固定 `/v1/models` 探测；按 provider 类型生成标准 probe：
  - OpenAI-compatible / DeepSeek / Mistral / Ollama 等默认 `GET /models`。
  - Anthropic 默认 `GET /v1/models`，带 `anthropic-version` 与 `x-api-key`。
  - Gemini 自动补 `/v1beta/openai` OpenAI-compatible base。
  - Azure / Bedrock 走最小 chat-style `POST` probe，`max_tokens/maxTokens=1`、`temperature=0`。
- 每个 provider 有默认低成本 probe model；channel `supported_models[0]` 优先覆盖默认模型，避免探测不存在的 deployment。
- Compile-time 标准 probe 统一声明 `max_cost_micros=25`；Plugin probe 继续使用 manifest `probe.max_cost_micros`。
- 新增 `provider_health_probe_total` 与 `provider_health_probe_duration_seconds`，标签固定为 `provider_type` / `outcome` / `status_bucket`，覆盖成功率、延迟与错误码分桶。
- Health checker 会把 probe 成功/失败与延迟写回 `ProviderRouter` 的 `ChannelMetrics`，`least_latency` 可利用巡检样本，而不只依赖真实请求热度。

验证命令：

```bash
cargo fmt --all -- --check
cargo test -p gate-server health_check::tests -- --nocapture
cargo test -p gate-server --test c1_routing health_checker -- --nocapture
cargo clippy --all-targets -- -D warnings
```

## P1.3 data-plane error shape unification

本轮把 P1.3 最后一项 error shape 收口为同一响应骨架：

- 统一响应体：`{ "error": { "code": "...", "type": "...", "message": "...", ... } }`；保留旧测试依赖的 `code`，新增 OpenAI-compatible `type`。
- 上游 auth：`authentication_error`，对客户端返回 502，避免暴露真实 provider key 细节。
- 上游 rate limit：`rate_limit_error`，保留 `retry_after_ms` 并写 `Retry-After` header。
- quota middleware：`quota_exceeded` + `type="quota_error"`，仍返回 429，并保留 `dimension` / `retry_after_ms`。
- model missing：OpenAI-compatible / Anthropic / Bedrock / HTTP Plugin mapper 均归一为 `model_not_found`。
- no healthy route：`route_chat_required` 在有 project routing 但无健康/兼容 channel 时返回 normalized `no_healthy_channel`，不再静默 fallback 到全局 provider。
- channel key failure policy 从 chat / embeddings / images / audio 分散实现收束到 `provider_failure_policy`，channel cooldown、circuit breaker、metrics label 共用一套分类。

验证命令：

```bash
cargo fmt --all -- --check
cargo test -p gate-server --test chat_e2e
cargo test -p gate-server --test c1_routing route_chat_no_healthy_channel_returns_normalized_error -- --nocapture
cargo test -p gate-server --test quota_enforce rpm_quota_blocks_after_limit -- --nocapture
cargo test -p gate-server --test rate_limit_mw user_hits_429_after_quota_exhausted -- --nocapture
cargo test -p gate-providers --test custom_provider plugin_error_mapper_normalizes_model_not_found_and_policy_block -- --nocapture
cargo test -p gate-providers --all-targets
cargo clippy --all-targets -- -D warnings
```

## P1.3 `/v1/responses` thin adapter

本轮按 ROADMAP 的“先做 thin adapter 到 chat，不复刻完整 tool/state machine”收口 `/v1/responses`：

- 新增 `routes::responses`，在全量 router、gateway-only router 与 route manifest 中注册 `POST /v1/responses`。
- `ResponsesRequest` 支持常用迁移面：`model`、string / item-array `input`、`instructions`、`stream`、`temperature`、`top_p`、`max_output_tokens`、`tools`、`tool_choice` 与 flattened extra。
- adapter 把 Responses input 转为 `ChatRequest.messages`：`instructions` → system message，string input → user message，`input_text` / `input_image` parts → chat text / image parts。
- 非流式 Responses 复用 chat provider route / adapt / retry / billing / quota settle / TPM record / channel success/failure 链路，返回 `object="response"`、`status="completed"`、`output[]` 与 `output_text`。
- 流式 Responses 复用 chat stream，上游 chat chunk 映射为 `response.output_text.delta` SSE，并在尾帧输出 `response.completed`；usage 末帧继续用于 billing / quota settle。
- 不实现 Responses 完整 state machine、stored response、conversation item lifecycle、parallel tool orchestration；这些仍按 vNext 评估。

验证命令：

```bash
cargo fmt --all -- --check
cargo test -p gate-server --test chat_e2e responses -- --nocapture
cargo test -p gate-server --test runtime_modes
cargo test -p gate-server --test billing_e2e
cargo clippy --all-targets -- -D warnings
```

## P1.3 `/v1/audio/speech` / `/v1/audio/transcriptions` billing/quota loop

本轮把 P1.3 audio endpoints 从单一 fallback provider 代理推进为可对账的 data-plane 闭环：

- ProviderRouter 新增 `route_audio`，按 project default group / fallback group / channel strategy 选择 audio-capable channel。
- 当前 audio runtime 仅支持 compile-time OpenAI-compatible `AudioProvider`，因此会过滤 plugin channel（即使 manifest 声明 `audio=true`），避免路由到尚未实现的 runtime adapter。
- 路由结果贯通 `resolved_model` 与 `channel_id`：model alias / channel `model_mapping` 会写回 upstream request，billing event 与 request log 使用实际模型和命中 channel。
- `least_conn` acquire 仍在 provider/key 构造成功之后执行；audio 成功 / provider error 路径都会 release。
- `/v1/audio/speech` 成功响应生成 `Usage`：token 维度为 0，`raw_usage.endpoint="audio.speech"`，并记录 `tts_characters`、`response_bytes`、`voice`、`response_format`、`speed`。
- `billing_emit` 会把 raw `tts_characters` 写入 `CostContext.tts_characters`，因此 `per_character_tts` pricing rule 可直接计费。
- `/v1/audio/transcriptions` 初版按 `per_request` 计费；由于 OpenAI-compatible multipart 响应不带真实 duration，raw usage 先保留 `audio_bytes`、`filename`、`language` 与 `metering="per_request"`，后续若上游返回 duration 再升级为 `per_minute_audio`。
- Billing outbox → `commit_usage` 后能落 `usage_records`、`request_events` 与 request log read model；audio 请求在 read model 中 token 为 0，但成本和 channel 归属可对账。
- quota middleware 支持解析 JSON `AudioSpeechRequest`，按 input 字符数估算 budget pre-debit；handler 完成后按 `tts_characters` settle。multipart transcription 暂用默认保守预估，handler 成功后按 STT per-request 初版口径 settle。
- provider error 不再包装为 `internal`，统一走 `AppError::Provider`，并同步 channel key failure cooldown / circuit breaker 统计与 upstream error metrics。

验证命令：

```bash
cargo fmt --all -- --check
cargo test -p gate-server middleware::quota::tests -- --nocapture
cargo test -p gate-server --test billing_e2e audio_speech_apikey_emits_usage_event -- --nocapture
cargo test -p gate-server --test billing_e2e audio_transcription_apikey_emits_usage_event -- --nocapture
cargo test -p gate-server --test quota_predebit audio_speech_predebit_settles_and_blocks_when_budget_exceeded -- --nocapture
cargo test -p gate-server --test billing_e2e
cargo test -p gate-server --test quota_predebit -- --nocapture
cargo test -p gate-providers --all-targets
cargo clippy --all-targets -- -D warnings
```

## P1.3 `/v1/images/generations` adapter/billing loop

本轮把 P1.3 `/v1/images/generations` 从单一 fallback provider 代理推进为可对账 data-plane 闭环：

- ProviderRouter 新增 `route_image`，按 project default group / fallback group / channel strategy 选择 image-capable channel。
- 当前 image runtime 仅支持 compile-time OpenAI-compatible `ImageProvider`，因此会过滤 plugin channel（即使 manifest 声明 `image=true`），避免路由到尚未实现的 runtime adapter。
- 路由结果贯通 `resolved_model` 与 `channel_id`：model alias / channel `model_mapping` 会写回 upstream request，billing event 与 request log 使用实际模型和命中 channel。
- `least_conn` acquire 移到 provider/key 构造成功之后，避免构造失败泄露 inflight 计数；image 成功 / provider error 路径都会 release。
- 成功响应按 billable image units 生成 `Usage`：`image_units = max(request.n, returned_images, 1)`，token 维度为 0，`raw_usage.endpoint="images.generations"`。
- `billing_emit` 会把 image request 的 `quality` / `size` 写入 `CostContext`，因此 `per_image` pricing rule 的 `conditions` 可按图片质量和尺寸命中。
- Billing outbox → `commit_usage` 后能落 `usage_records`、`request_events` 与 request log read model；image 请求在 read model 中 token 为 0，但成本和 channel 归属可对账。
- quota middleware 支持解析 `ImageGenerationRequest`，按默认 `$0.08/image` 估算 budget pre-debit；handler 完成后按 billable image units settle。
- provider error 不再包装为 `internal`，统一走 `AppError::Provider`，并同步 channel key failure cooldown / circuit breaker 统计与 upstream error metrics。

验证命令：

```bash
cargo fmt --all -- --check
cargo check -p gate-server -p gate-providers
cargo test -p gate-server middleware::quota::tests -- --nocapture
cargo test -p gate-server --test billing_e2e images_apikey_emits_usage_event -- --nocapture
cargo test -p gate-server --test quota_predebit images_predebit_settles_and_blocks_when_budget_exceeded -- --nocapture
cargo test -p gate-server --test billing_e2e embeddings_apikey_emits_usage_event -- --nocapture
cargo test -p gate-server --test quota_predebit embeddings_predebit_settles_and_blocks_when_budget_exceeded -- --nocapture
```

说明：`--no-git --source .` 会扫描 `.env` 与 `target/` 等 gitignored 本地文件；用于泄露排障时有价值，但不代表仓库可提交内容。本轮仓库口径采用 git history + tracked/unignored working tree 两条扫描。

## Plugin secret slots

本轮把 P1.1.2 的 “Secret 来源统一”、`hmac`、`aws_sigv4` 与 `oauth_client_credentials` 从 TODO 收口为代码路径：

- `CustomHttpProvider::new_with_secret_slots` 接收 slot map，`new_with_opts` 继续兼容旧 primary API key。
- `ProviderRouter::resolve_secrets_for_channel` 读取同一 channel 的 active `channel_keys`，按 `label` 归一为 secret slot 并用 `EnvelopeKms` 解密。
- `primary` / `api_key` / 空 label 保持旧主密钥语义；非 plugin provider 仍只使用 primary。
- repo/crypto 缺失或 DB 无 active key 时回退 env：`KOOIX_CH_<CODE>_KEY`、`KOOIX_API_KEY`、`KOOIX_PLUGIN_SECRET_<SLOT>`、`AWS_SECRET_ACCESS_KEY`。
- `auth.strategy = "hmac"` 支持 method/path/query/body_sha256/timestamp/nonce 签名 payload，使用 `secret_slot` 做 HMAC-SHA256，并自动注入 timestamp / nonce / signature header。
- `auth.strategy = "aws_sigv4"` 支持 AWS Signature Version 4 canonical request / string-to-sign / signing key，自动注入 `Authorization` / `x-amz-date` / `x-amz-content-sha256` / 可选 `x-amz-security-token`。
- Bedrock Converse preset 默认切到 `aws_sigv4`，不再注入临时 `X-Amz-Access-Key` / `X-Amz-Secret-Key` header。
- `auth.strategy = "oauth_client_credentials"` 支持向 HTTPS `token_url` 发送 client credentials form，用 `client_id_slot` / `client_secret_slot` 换取 access token，缓存到过期前并注入 `Authorization: Bearer <token>`。
- Admin channel test 对 plugin provider 改为传完整 secret slot map；`channel_keys.alias` 新增 slot 字符集校验，避免 UI 写入运行期无法引用的 slot。

验证命令：

```bash
cargo test -p gate-providers router_db_key_decrypt_roundtrip -- --nocapture
cargo test -p gate-providers router_secret_slots_use_channel_key_labels -- --nocapture
cargo test -p gate-providers plugin_auth_uses_explicit_secret_slot_map -- --nocapture
cargo test -p gate-providers plugin_auth_hmac_signs_method_path_body_timestamp_nonce -- --nocapture
cargo test -p gate-providers parses_hmac_auth_manifest_defaults_and_payload_template -- --nocapture
cargo test -p gate-providers hmac_rejects_unknown_payload_template_variable -- --nocapture
cargo test -p gate-providers plugin_auth_aws_sigv4_signs_bedrock_request -- --nocapture
cargo test -p gate-providers parses_aws_sigv4_auth_manifest_defaults -- --nocapture
cargo test -p gate-providers bedrock_preset_defaults_to_aws_sigv4_without_fake_secret_headers -- --nocapture
cargo test -p gate-providers oauth -- --nocapture
cargo test -p gate-providers plugin_env_secret_slots_include_named_plugin_secrets -- --nocapture
cargo test -p gate-providers plugin -- --nocapture
cargo clippy -p gate-providers --all-targets -- -D warnings
cargo clippy -p gate-server --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## Plugin Auth 前端表单

本轮把 P1.1.2 的前端 channel auth strategy 配置从原始 manifest 手填推进为可 lint 表单：

- `web/src/lib/components/channels/PluginAuthEditor.svelte`：创建 / 编辑 channel 共用的 Auth Strategy editor。
- `web/src/lib/plugin-presets.ts`：新增 `PluginAuthForm`、默认 preset auth、manifest → form round-trip、`buildPluginAuthManifest` 本地 lint 与 auth 合并逻辑。
- `web/src/routes/channels/+page.svelte`：Plugin provider 创建 / 编辑抽屉按 strategy 展示最小字段，保存前合并 auth 到 manifest；“本地 lint”按钮复用同一构造链。
- 支持策略：`bearer`、`api_key_header`、`api_key_query`、`basic`、`custom_headers`、`hmac`、`aws_sigv4`、`oauth_client_credentials`、`none`。
- 本地 lint 限制：secret slot 仅允许 `[a-zA-Z0-9_-]`；OAuth `token_url` 必须 HTTPS，本地仅放行 `localhost` / `127.0.0.1`；`expiry_skew_seconds` 限制 0-3600；custom headers 必须是非空 JSON object。

验证命令：

```bash
npm --prefix web run check
npm --prefix web test -- plugin-presets
```

## Plugin Request Mapping DSL

本轮把 P1.1.3 的 request mapping 从基础模板推进到可覆盖私有 deployment 的 DSL：

- `request.path` / `request.query` / `request.headers` / `request.body` 模板新增 `tools`、`tool_choice`、`metadata.*`、`extra.*`，body 也支持整段 `metadata` / `extra`。
- 整段占位继续保留 JSON 原类型；缺失、`null`、空字符串、空数组、空对象会在 query/header/body object 中自动跳过，避免私有上游拒绝未知空字段。
- Header 仍保留分域白名单：`{{messages}}` 等大 payload 不能塞进 header，manifest 加载时直接拒绝。
- Anthropic Messages 与 Bedrock Converse preset 继续通过 `adapt_chat_request` 做 message transform，覆盖 system prompt、multimodal parts、tool calls / tool results 基础映射。
- Plugin channel 的 `model_mapping` 可同时保留 `plugin` manifest 与 `models` / `model_aliases` / `deployments` 映射，路由顺序为 project model alias → channel deployment mapping → plugin request 模板。

验证命令：

```bash
cargo test -p gate-providers plugin -- --nocapture
cargo test -p gate-server --test c1_routing plugin_manifest_channel_model_mapping_rewrites_deployment_path -- --nocapture
cargo test -p gate-server --test c1_routing full_chain_rewrites_model_from_alias_and_channel_mapping -- --nocapture
```

## Plugin Response / Usage Mapping

本轮把 P1.1.4 的非流式 response / usage 映射收口为稳定 evaluator 与可对账响应字段：

- 字段路径从简单 dot path 扩展为 `nested.object`、`array.0.index`、`path.a|path.b|default:<json>` first non-null fallback。
- 非流式 response 新增 `reasoning_content_path`、`tool_calls_path`、`request_id_path`、`metadata_path`；`request_id` 与 `upstream_metadata` 会保留在 `ChatResponse`，便于日志 / replay / vendor 对账。
- Usage 新增 `reasoning_tokens_path`、`image_units_path`、`audio_seconds_path`、`raw_path`；`raw_path` 保存 vendor 原始 usage metadata。
- 字段缺失按 0 / fallback 处理；usage 类型不匹配会返回 decode error，避免静默错计费。
- pricing 管理页维度改为后端 `pricing_rules` 实际消费的维度名：`per_image`、`per_minute_audio`、`reasoning_tokens` 等，避免 UI 写入旧维度后计费引擎无法匹配。
- Billing emit 改为直接读取 `pricing_rules` 并用 `compute_cost(CostContext, rules)`，不再只走 legacy `ModelPricing` 的 input/output/cached 三列；reasoning/image/audio 映射出来后可被同名 pricing dimension 消费。

验证命令：

```bash
cargo test -p gate-providers response_mapping -- --nocapture
cargo test -p gate-providers plugin_maps_response_paths_fallback_tool_calls_metadata_and_usage_units -- --nocapture
cargo test -p gate-server --test billing_e2e non_stream_usage_event_keeps_raw_and_multimodal_cost_dimensions -- --nocapture
cargo check -p gate-server
```

## Plugin SSE Normalizer / Replay Harness

本轮把 P1.1.5 的 SSE normalizer 从共享 decoder 推进到 manifest-driven 产品能力：

- `stream.ignore_events` / `stream.done_events`：按 SSE `event:` 名称跳过 heartbeat / ping 或结束分流。
- `stream.done_path` / `stream.done_values`：支持 vendor done object，例如 `{"type":"message_stop"}`，不再只识别 `[DONE]` / `EOF` raw token。
- `stream.tool_calls_path`：私有 tool call delta array 直接映射到 `ChatDelta.tool_calls`。
- `UsageManifest::should_emit_stream_usage`：usage-only 末帧即使只有 prompt / cached / reasoning / raw usage 也可输出；Anthropic output-only streaming 仍避免 message_start prompt-only 帧提前对外暴露。
- `gate_providers::replay_plugin_sse`：后端 / CLI / UI 共用同一回放核心。
- `POST /v1/admin/plugin-manifest/replay`：平台管理员可上传 manifest + raw SSE，返回 OpenAI-compatible chunks。
- `kgctl plugin replay manifest.json --sse sample.sse`：本地 fixture 回放，不需要启动 gate-server。
- Channel 创建 / 编辑抽屉新增 `SSE replay preview`，可直接粘贴 raw SSE 预览归一 chunks。
- `/v1/chat/completions` 流式 billing guard 改为缺 usage 末帧时生成 estimated usage 并写 outbox，`raw_usage.estimated=true`，避免静默漏扣。

验证命令：

```bash
cargo test -p gate-providers replays_manifest_driven_sse_events_tool_calls_usage_and_done_object -- --nocapture
cargo test -p gate-providers plugin_normalizes_event_split_tool_delta_usage_and_vendor_done -- --nocapture
cargo test -p gate-server --test billing_e2e stream_without_usage_frame_emits_estimated_usage_event -- --nocapture
cargo run -q -p kgctl -- plugin replay /tmp/kgctl-plugin-replay/manifest.json --sse /tmp/kgctl-plugin-replay/sample.sse
npm --prefix web test -- plugin-presets
```

## P1.1.7 Manifest Builder / Debugger

本轮把 Manifest Builder / Debugger 从分散的 textarea + replay 入口推进为可验收的 7 步创建流，并补齐 CLI golden fixture 回放：

- `kgctl plugin test`：用 `CustomHttpProvider` 对真实 / mock 上游发一次 non-stream chat，输出归一后的 `ChatResponse`，默认 API key 可读 `KOOIX_PLUGIN_TEST_API_KEY`。
- `kgctl plugin export`：把 manifest、可选 non-stream response sample、raw SSE 与 replay 后的 `expected_chunks` 导出为 v1 golden fixture。
- `kgctl plugin import --verify`：校验 fixture manifest，并重放 raw SSE 与 `expected_chunks` 比对；生成型 `chatcmpl-*` id 会在比较时归一，避免非确定性破坏 golden。
- Channel 创建抽屉新增 7 步 builder：preset/custom → auth → request mapping → response sample 点选字段 → raw SSE replay → probe/test 参数 → 保存并可自动 `addGroupBinding` 加入 group。
- `web/src/lib/plugin-presets.ts` 新增 `PluginBuilderDraft`、`buildPluginBuilderManifest`、`suggestResponsePaths`，让 response sample 可自动建议 `content_path` / `finish_reason_path` / usage paths，也支持手动点选覆盖。
- 前端 `ProbeResponse` 类型补齐 `probe_model` 与 `max_cost_micros`，与后端 P1.1.6 返回结构对齐。
- 新增 `crates/gate-server/tests/channel_plugin_e2e.rs`，覆盖 replay → create plugin channel → group binding 的控制面闭环。

验证命令：

```bash
cargo test -p kgctl plugin_ -- --nocapture
cargo test -p gate-server --test channel_plugin_e2e -- --nocapture
npm --prefix web test -- plugin-presets
npm --prefix web run check
```

## P1.2 Provider Capability Matrix

本轮把 P1.2 的 capability matrix 从路线项落成 runtime / API / UI 共享契约：

- `gate_providers::ProviderCapabilities` 成为内置 Provider 与 HTTP Plugin manifest v1 共用字段，覆盖 `chat`、`streaming`、`tools`、`embeddings`、`image`、`audio`、`vision`、`json_mode`、`batch`。
- `PluginManifest::apply_preset` 会把 preset 的 truthy capability 默认值并入 manifest；旧 v0 / 简写 preset 仍可自动升级。
- Router 新增 `route_chat`，会按请求实际需求跳过不满足 `streaming` / `tools` / `vision` / `json_mode` 的 channel，并在 route decision trace 记录 `missing_capability:*`。
- Embedding route 改为读取 capability matrix，只选择声明 `embeddings=true` 且当前已有 embedding runtime 的内置 Provider。
- Admin Channel / Group binding API 返回 `capabilities`，控制台在 Channel 列表、创建 / 编辑抽屉和 Group binding 表展示 capability chips。
- Plugin preset 增加 Base URL 建议与本地 / 自托管 OpenAI-compatible 变体：`vllm`、`lm_studio`、`ollama_openai`、`localai`、`xinference`。
- Bedrock Converse 保持 `aws_sigv4` 正式鉴权，capability 先按保守 `chat` / `streaming` 声明。

验证命令：

```bash
cargo test -p gate-providers capability -- --nocapture
cargo test -p gate-providers preset_defaults_fill_capabilities -- --nocapture
cargo test -p gate-providers openai_compatible_variant_presets_parse -- --nocapture
cargo test -p gate-providers route_chat_records_capability_skip_reason -- --nocapture
cargo test -p gate-server --test c1_routing route_chat_skips_channel_missing_requested_capability -- --nocapture
cargo test -p gate-server --test auth_flow admin_can_create_plugin_channel_with_provider_preset_manifest -- --nocapture
cargo test -p gate-server --test channel_plugin_e2e plugin_manifest_builder_flow_creates_fixture_channel_and_group_binding -- --nocapture
npm --prefix web test -- plugin-presets
npm --prefix web run check
```

## P1.3 `/v1/models` capability aggregation

本轮把 P1.3 第一项从路线项落成 data-plane API 行为：

- `GET /v1/models` 从所有 `active + healthy` channel 的 `supported_models` 聚合模型，disabled / unhealthy channel 不再出现在对外模型列表。
- 每个 `ModelInfo` 新增可选 `capabilities` 字段，shape 复用 `ProviderCapabilities`：`chat`、`streaming`、`tools`、`embeddings`、`image`、`audio`、`vision`、`json_mode`、`batch`。
- 同一模型由多个 channel 承载时，capability 以 truthy OR union 聚合，代表当前至少有一条健康运行链可提供该能力。
- Plugin channel capability 以 `model_mapping.plugin` manifest v1 解析结果为准；manifest 无效时回退 provider 默认 capability，保持旧渠道兼容。
- 前端 `ModelInfo` 类型同步加入 `capabilities?: ProviderCapabilities`，避免 OpenAI-compatible model list 扩展字段造成 TS 漂移。

验证命令：

```bash
cargo test -p gate-server --test perf_smoke models_endpoint_aggregates_healthy_channel_capabilities -- --nocapture
cargo test -p gate-server --test perf_smoke -- --nocapture
```

### Side fix: quota inflight insert/settle race

全量 `cargo test --workspace` 暴露 `request_id_is_shared_by_quota_inflight_and_billing_outbox` 偶发失败：`quota_enforce` 把 `inflight_requests` insert 放进后台 task，短请求可能先完成 handler settle/delete，随后后台 insert 才落库，导致同一 `x-request-id` 残留一条 inflight 记录。

修复：`quota_enforce` 在把 `InflightGuards` 交给 handler 前同步 best-effort insert；DB 写失败仍 fail-open 继续请求，但不再允许 insert/delete 生命周期乱序。

验证命令：

```bash
cargo test -p gate-server --test quota_predebit -- --nocapture
cargo test --workspace
```

## P1.3 `/v1/embeddings` billing/quota loop

本轮把 P1.3 `/v1/embeddings` 从简单代理补成可对账的 data-plane 闭环：

- Embedding route 走 `ProviderRouter::route_embedding`，只选择 `active + healthy` 且 capability 声明 `embeddings=true` 的内置 embedding provider channel。
- 路由结果贯通 `resolved_model` 与 `channel_id`：model alias / channel `model_mapping` 会写回 upstream request，billing event 与 request log 使用实际模型和命中 channel。
- `least_conn` 策略在 embedding 选中 channel 后 acquire，并在成功 / provider error 路径 release，避免 inflight 计数漂移。
- 成功响应读取 upstream `EmbeddingResponse.usage`：`prompt_tokens` 使用上游值，`completion_tokens=0`，`total_tokens` 至少不小于 prompt tokens。
- Billing outbox 写入 `raw_usage.endpoint="embeddings"`；consumer `commit_usage` 后能落 `usage_records`、`request_events`，并可通过 `PgRequestLogRepo.find_by_request_id` 读到 request log read model。
- quota middleware 支持解析 `EmbeddingRequest`：按 input 字符数 / 4 估算 pre-debit；handler 完成后用实际 `usage.total_tokens` settle，多退少补。
- provider error 不再包装为 `internal`；auth、rate limit、invalid request、policy、upstream、network、decode、config 与 mapped error 进入统一 `AppError::Provider` shape，同时写 channel key failure cooldown / circuit breaker 统计与 upstream error metrics。
- embedding 暂不走全局 provider fallback：`AppState.provider` 是 `Arc<dyn Provider>`，无法安全下转 `EmbeddingProvider`；没有匹配 embedding channel 时返回清晰 `bad request: no embedding channel found for model ...`。

验证命令：

```bash
cargo fmt --all -- --check
cargo test -p gate-server middleware::quota::tests -- --nocapture
cargo test -p gate-server --test billing_e2e embeddings_apikey_emits_usage_event -- --nocapture
cargo test -p gate-server --test quota_predebit embeddings_predebit_settles_and_blocks_when_budget_exceeded -- --nocapture
cargo test -p gate-server --test quota_predebit embedding_request_id_is_shared_by_quota_inflight_and_billing_outbox -- --nocapture
cargo test -p gate-server --test chat_e2e
cargo test -p gate-server --test billing_e2e
cargo test -p gate-server --test quota_predebit -- --nocapture
cargo test -p gate-providers --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --workspace
npm --prefix web run check
npm --prefix web test
gitleaks detect --source . --redact --verbose
tmp=$(mktemp -d) && git ls-files -co --exclude-standard -z | tar --null -T - -cf - | tar -C "$tmp" -xf - && gitleaks detect --source "$tmp" --no-git --redact --verbose
```

## P1.6 Quota / Policy engine

本轮把 P1.6 从 roadmap 项落成完整 policy engine：

- quota schema 增加 `mode=enforce|dry_run`，dimension 增加 `lifetime_budget_usd`；历史 `quotas_dimension_check` 迁移会先安全 drop 再重建，避免空库 / 旧库约束重名。
- middleware 支持 `rpm`、`tpm`、`concurrent`、`daily_budget_usd`、`monthly_budget_usd`、`lifetime_budget_usd`、`lifetime_tokens`，并按 `model_filter` 精确 / 简单 glob 过滤规则。
- TPM sliding window 支持按 estimated token amount 多单位记账；`lifetime_tokens` settle 使用真实 usage tokens，不再混用 cost micros。
- `mode=dry_run` 只 peek 当前 Redis 用量，记录 `quota_dry_run_total` 与 would-deny tracing，不扣 Redis、不拦截。
- control-plane 新增 quota explain / reconcile：`/v1/orgs/:org_id/quotas/explain` 返回命中规则、当前消耗、估算量、剩余量、would-deny 与 reset；`/reconcile` 对比 Redis counter 与 PG usage projection。
- Quota UI 重铸为 policy 工作台：支持 org/project/api_key/user scope、model filter、enforce/dry-run、lifetime budget、explain 面板与 Redis/PG 对账面板。
- InMemoryMembershipRepo 补 `list_org_members`，让 user scope quota 在 dev/test 的 list / delete / reconcile 链路与 PG 行为一致。

已跑验证：

```bash
cargo clean -p gate-storage
cargo test -p gate-storage --test quota_repo
cargo test -p gate-server --test quota_enforce
cargo test -p gate-server --test quota_predebit
npm --prefix web run check
```

剩余全量门禁继续在本阶段末尾统一跑：`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`npm --prefix web test`、`npm --prefix web run build`、`gitleaks` 双扫描。

## P1.7 Identity / Session 管理

本轮把 P1.7 中的 Session 管理从无状态 TODO 落成可运维闭环：

- `gate-storage` 新增 `UserSessionRepo`，PG / InMemory 双实现对齐 `user_sessions` 表；refresh token 只以 SHA-256 hash 存储。
- `/v1/auth/login` 与 SSO callback 会创建 session；`/v1/auth/refresh` 校验 session 未撤销/未过期并原子轮转 refresh hash，旧 refresh token 重放返回 `token_invalid`。
- `/v1/auth/logout` 撤销当前 session，平台管理员可通过 `/v1/admin/users/:id/sessions` 查看活跃 session，并撤销单个或全部 session。
- 前端 `/admin/users` 增加 Session 面板；`apiFetch` refresh 流程保存服务端返回的新 refresh token，避免 token rotation 后继续使用旧 token。
- 文档同步 `README.md`、`DESIGN.md`、`CHANGELOG.md`、`ROADMAP.md`、`web/README.md`；未完成的全局 JWT rotation / `JwtRing` 保持在 P1.7 待办。

阶段验证命令：

```bash
cargo fmt --all
cargo check -p gate-storage -p gate-server
cargo test -p gate-storage session
cargo test -p gate-server --test auth_endpoints_e2e
cargo test -p gate-server --test admin_users_e2e
cargo test -p gate-server --test sso_flow
npm --prefix web run check
npm --prefix web test -- api
```

## P1.7 Identity / JwtRing

本轮把 P1.7 剩余的全局 JWT rotation / `JwtRing` 从路线图落成部署可用能力：

- `gate-auth::jwt::JwtRing` 成为 `JwtIssuer` 兼容实现：新 access / refresh token 只用 primary secret 签发，验签按 primary → previous secrets 顺序尝试。
- `gate-server` 启动读取 `KOOIX_JWT_SECRET` + 可选 `KOOIX_JWT_PREVIOUS_SECRETS`；配置旧 secret 时会记录 rotation verification window active。
- `kgctl env` 与 `kgctl doctor` 增加 `KOOIX_JWT_PREVIOUS_SECRETS`：未配置为 OK；配置时必须是逗号分隔 base64 secret，且每项解码后至少 32B。
- Security runbook 区分计划轮换与泄露处置：计划轮换保留旧 key 验签窗口；泄露时清空 previous、撤销 session 并强制重新登录。
- 部署示例同步 Helm / Terraform / docker-compose；关键文档同步 `README.md`、`DESIGN.md`、`CHANGELOG.md`、`ROADMAP.md`、`RELEASE.md`、`crates/kgctl/README.md`。

阶段验证命令：

```bash
cargo fmt --all
cargo test -p gate-auth jwt
cargo test -p gate-server --test auth_flow me_
cargo test -p kgctl --test cli doctor
```

## P1.7 Identity / SSO Provider UI

本轮把 P1.7 的 SSO provider UI 完整化落成控制面能力：

- `IdentityProviderRepo` 补齐 list/create/update/soft_delete，`IdentityProviderRecord` 显式暴露 `metadata`，用于保存 `redirect_policy`。
- Admin API 新增 `GET/POST /v1/admin/identity-providers`、`PUT/DELETE /v1/admin/identity-providers/:id`、`POST /v1/admin/identity-providers/discover`，全部要求 `Permission::PlatformAdmin`，并写入 `identity_provider.*` audit。
- `client_secret` 创建 / 轮换时用 `EnvelopeKms` 加密，AAD 固定 `gate_crypto::aad::idp_secret(provider_id)`；API response 与 audit 不回显明文或密文。
- 公开 `GET /v1/auth/sso/providers` 只返回 enabled 平台级 Provider 的 `name/slug`；登录页改为动态展示 SSO 入口，不再硬编码 `google`。
- 控制台新增 `/admin/sso`：支持 OIDC discovery、邮箱域 allowlist、claim mapping、JIT auto-create、auto-join role、enabled 切换与 redirect policy 编辑。
- SSO start/callback 增加 redirect policy enforcement：相对路径由 `allow_relative` 控制；绝对 URL 必须命中 `allowed_origins`；scheme-relative URL、`javascript:` 与未授权 origin 返回 `bad_request`。
- Route manifest 与生成的 `web/src/lib/api/route-manifest.ts` 同步新增 SSO 管理/公开路由，并补回此前已服务但未列入 manifest 的 channel drain 运维路由。

阶段验证命令：

```bash
cargo fmt --all
cargo test -p gate-storage identity
cargo test -p gate-server --test sso_admin_e2e
cargo test -p gate-server --test sso_flow
npm --prefix web run check
node scripts/check-route-manifest.mjs
node scripts/generate-route-types.mjs --check
```

## P1.7 Identity / SCIM Evaluation

本轮把 P1.7 中的 SCIM 评估从路线图 TODO 收敛为可执行 vNext 边界，长期文档落在 `docs/scim-evaluation.md`：

- 结论：当前只完成 SCIM 2.0 评估，不声明已有 SCIM runtime endpoints；后续实现必须作为 Org-scoped inbound provisioning connector。
- 用户同步：以 email 归一化匹配 `users.email`，`externalId` 进入独立 SCIM binding；新建用户不设置密码，默认走 SSO；deprovision 映射为 suspend user + revoke refresh sessions。
- Group → role mapping：SCIM Group 不直接等同 Kooix role，必须通过管理员显式 mapping 投影到 Org / Project role；Project mapping 必须带 Org 上下文并校验 `projects.org_id`。
- 安全边界：connector token 只存 hash，mutation 写 `scim.*` audit；SCIM 不授予 `PlatformRole`、不创建 Org / Project、不能撤销本地手工 Owner / Admin。
- 差距清单：需要 vNext migration（connection / user link / group mapping / membership grants）、repo、route manifest、UI mapping 页、source-aware membership revoke。
- 关键文档同步 `README.md`、`DESIGN.md`、`CHANGELOG.md`、`ROADMAP.md`、`docs/README.md`。

阶段验证命令：

```bash
git diff --check
rg -n "SCIM|scim|group → role|Session 管理" README.md DESIGN.md ROADMAP.md CHANGELOG.md docs
```

## P1.7 Identity / Invitation Flow

本轮把 P1.7 的邀请流从 schema TODO 落成 Org / Project 成员接入闭环：

- `gate-storage` 新增 `InvitationRepo`，PG / InMemory 双实现对齐既有 `invitations` 表；明文 token 只在创建响应返回一次，存储层只保存 `SHA-256(token)`。
- Admin API 新增 Org invitation create/list/revoke 与 Project invitation create/list/revoke；Org 入口要求 `OrgMemberInvite` / `OrgMemberRemove`，Project 入口要求 `ProjectMemberInvite` / `ProjectMemberRemove`。
- 公开 `POST /v1/invitations/preview` 与 `POST /v1/invitations/accept`：preview 只暴露邮箱、scope、role、过期与状态；accept 校验 token pending、邮箱匹配、用户 active 或新建密码用户后写 membership。
- 过期 / 撤销 / 已接受邀请均无法再次接受；accept 使用条件更新阻断重放。
- Project invite accept 会重新读取 `projects.org_id` 并写入带 `(OrgId, ProjectId)` 上下文的 project membership，延续跨 Org project ID 重放防线。
- 控制台在 `/orgs/[orgId]/projects` 增加 Org invite 面板，在 `/orgs/[orgId]/projects/[projectId]` 增加 Project invite 面板；新增 `/invite/accept` 公开接受页。
- Route manifest 与生成的 `web/src/lib/api/route-manifest.ts` 同步新增 8 条 invitation 路由；关键文档同步 `README.md`、`DESIGN.md`、`CHANGELOG.md`、`ROADMAP.md`、`web/README.md`。

阶段验证命令：

```bash
cargo fmt --all
cargo test -p gate-server --test invitations_e2e
cargo test -p gate-storage invitation_repo_create_list_accept_and_revoke
npm --prefix web run check
node scripts/check-route-manifest.mjs
node scripts/generate-route-types.mjs --check
```

## P1.8 Plugin Ecosystem / Manifest Registry

本轮把 P1.8 的 Manifest registry 首项从路线图 TODO 落成可导入/导出的注册表边界：

- 新增 `examples/manifest-registry/registry.json`，把官方 `openai-compatible` / `private-auth-field-map-sse` 与社区 `openai-compatible-lite` manifest 样本登记为 registry entries。
- 每条 entry 固定 `id`、`name`、`version`、`author`、`source`、`manifest_path`、`sha256`、`signature.kind/value` 与 `compatibility.min_gate_version/max_gate_version/manifest_schema`。
- `kgctl plugin registry list` 默认读取官方/社区 registry，支持 `--json` 输出给后续 UI / automation 使用。
- `kgctl plugin registry package` 可把私有 manifest、README、security.md 与可选 fixtures 打成 package JSON，记录版本、作者、签名、兼容范围与 manifest digest。
- `kgctl plugin registry import` 把 package 导入 private namespace，写入 `private/<namespace>/<id>/<version>/manifest.json` / `README.md` / `security.md` / `fixtures/`，并更新 registry index；unsigned package 必须显式 `--allow-unsigned`。
- `kgctl plugin registry export` 默认隐藏 private entries，只有 `--include-private` 才导出私有索引，避免内部 provider 元数据误外发。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`docs/plugin-manifest.md`、`crates/kgctl/README.md`、`examples/README.md`、`docs/README.md`。

阶段验证命令：

```bash
cargo fmt --all
cargo check -p kgctl
cargo test -p kgctl plugin_registry
cargo run -p kgctl -- plugin registry list --json
cargo run -p kgctl -- plugin registry package --id test-private --name "Test Private" --version 1.0.0 --author tester --manifest examples/manifests/openai-compatible.json -o /tmp/package.json
```

## P1.8 Plugin Ecosystem / Manifest Package Spec

本轮把 P1.8 的 Manifest package 规范从 registry JSON 打包能力补齐为目录形态规范与可验证样本：

- 新增 `examples/manifest-packages/private-auth-field-map-sse/` 作为标准 package 样本，固定包含 `package.json`、`manifest.json`、`fixtures/`、`README.md`、`security.md`。
- `fixtures/` 同时保留请求样本 `request.json`、非流式响应样本 `non-stream-response.json`与包含 raw SSE/expected chunks 的 golden 回放文件 `private-auth-field-map-sse.fixture.json`。
- `package.json` 记录 package metadata、兼容范围、签名状态与 contents 路径；`manifest.json` 与 registry 中同名官方 sample 保持 digest 一致。
- `README.md` 说明接入顺序、secret slot 与上游路径；`security.md` 明确 secret 不落包、relative path、egress 边界、大小限制与 fixture 发布检查。
- `kgctl plugin package lint <dir> --verify --json` 校验目录规范、manifest lint、README/security 必要声明、fixtures 存在性、fixture manifest 一致性与 SSE golden replay。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`docs/plugin-manifest.md`、`crates/kgctl/README.md`、`examples/README.md`、`docs/README.md`。

阶段验证命令：

```bash
cargo fmt --all
cargo check -p kgctl
cargo test -p kgctl plugin_package -- --nocapture
cargo run -p kgctl -- plugin package lint examples/manifest-packages/private-auth-field-map-sse --verify --json
```

## P1.8 Plugin Ecosystem / Plugin Sandbox Boundary

本轮把 P1.8 的 Plugin sandbox 安全边界从字段/建议收敛为运行时强制边界：

- `security.outbound_allowlist` 产品化为 origin allowlist；为空时走默认 denylist，非空时 base URL、绝对 path 与 OAuth token URL 都必须命中 allowlist。
- 绝对 `request.path/chat_path` 仍默认禁用；显式启用时必须同时声明 `security.permissions.absolute_urls=true`，并继续拒绝 localhost、private/link-local/metadata host。
- OAuth client credentials 需要 `security.permissions.oauth_client_credentials=true`；`permissions.secret_slots` 非空时会校验 auth 实际使用的 secret slot 已声明。
- 自定义 reqwest DNS resolver 会拒绝解析结果里的内网/metadata IP；响应返回后再校验 `remote_addr`，补上 DNS rebinding / 运行时漂移防护。
- `header_redaction` 与默认敏感头合并，支持 redacted probe/debug request；网络错误 URL 会脱敏 key/token/secret/password query。
- `request.timeout_ms` 覆盖 channel timeout 并限制在 1..600000 ms；request body、response body 与 SSE event size limit 保持硬上限并补测试覆盖。
- 样本 manifest、registry manifest 与 directory package fixture 均补齐 `outbound_allowlist`、`header_redaction`、`permissions` 字段，并刷新 registry/package digest。
- 关键文档同步 `CHANGELOG.md`、`ROADMAP.md`、`DESIGN.md`、`docs/plugin-manifest.md`、`docs/security-runbook.md` 与 package `security.md`。

阶段验证命令：

```bash
cargo fmt --all
cargo test -p gate-providers plugin -- --nocapture
cargo test -p gate-providers custom_provider -- --nocapture
cargo test -p kgctl plugin_ -- --nocapture
cargo run -p kgctl -- plugin package lint examples/manifest-packages/private-auth-field-map-sse --verify --json
```

## P1.8 Plugin Ecosystem / WASM ABI vNext Design

本轮只做 `ROADMAP.md` 要求的 WASM 插件 ABI 设计稿，不实现 runtime：

- 新增长期关键文档 `docs/wasm-plugin-abi.md`，明确 WASM 是 vNext 受限 transform runtime，不替代 HTTP Plugin manifest v1。
- 设计稿覆盖 request transform、response transform、streaming transform 三个 phase，并规定 host 继续掌控 network egress、billing、quota、routing、fallback、audit 与 trace。
- Secret access API 采用 capability + slot 声明；raw secret access 默认禁用，优先用 host-managed auth / derived signing hostcall，并要求每次访问写 audit。
- Deterministic constraints 禁止 filesystem、network、env、thread、direct clock/random 与跨请求持久写入；时间和 nonce 只能由 hostcall 显式提供。
- Resource limits 覆盖 wall timeout、stream event timeout、WASM memory pages、fuel/input/output bytes 与 stream scratch state；超限进入 fail-closed 和 channel key failure policy。
- Audit / metrics / trace 仅允许低基数字段，避免 request_id、org_id、api_key_id、错误原文等高基数字段污染 metrics。
- `docs/README.md` 把该设计稿列为关键文档；`DESIGN.md` 与 `docs/plugin-manifest.md` 指向该 vNext ABI，`ROADMAP.md` 将 P1.8 WASM ABI 设计稿子项全部勾选。

阶段验证命令：

```bash
git diff --check
rg -n "WASM|request transform|response transform|streaming transform|secret access|deterministic|资源限制|audit" docs/wasm-plugin-abi.md ROADMAP.md DESIGN.md docs/plugin-manifest.md
```

## P1.9 Observability / Prometheus Metrics Naming

本轮把 ROADMAP 中 “Prometheus metrics 完整命名” 从既有局部指标收敛为固定生产指标命名：

- `gateway_requests_total{method,path,status,status_class}` 固定 request count；旧 `gate_requests_total` 暂保留兼容。
- `gateway_request_duration_seconds{method,path,status_class}` 固定请求 latency，并在 Prometheus exporter 中配置 true histogram buckets；旧 `gate_request_duration_seconds` 暂保留兼容。
- `gateway_upstream_errors_total{kind,provider_type,channel,model}` 覆盖 chat / responses / embeddings / images / audio provider failures，fallback channel 标记为 `fallback`。
- `quota_denies_total{dimension,scope_kind,mode}` 记录 enforce hard deny；dry-run 继续使用 `quota_dry_run_total`。
- `billing_outbox_lag_seconds` 记录 billing outbox pending age；`billing_outbox_enqueued_total` 记录 enqueue。
- `billing_settle_lag_seconds` 记录 usage settlement age；`usage_rollup_lag_seconds` 保持 read model freshness。
- `docs/observability-runbook.md` 增加 PromQL 入口；`crates/gate-server/tests/perf_smoke.rs` 对 `/metrics` 和 InMemory outbox lag 做 smoke 覆盖。

阶段验证命令：

```bash
cargo fmt --all
cargo check --workspace
cargo test -p gate-server --test perf_smoke -- --nocapture
rg -n "gateway_requests_total|gateway_request_duration_seconds|gateway_upstream_errors_total|quota_denies_total|billing_outbox_lag_seconds|billing_settle_lag_seconds" crates docs ROADMAP.md CHANGELOG.md
```
