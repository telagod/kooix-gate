# Kooix Gate · 设计文档

> 一个不会越长越烂的 LLM 网关基座。

## 0. 设计原则

1. **底盘优先**：身份、权限、配额、路由——这四块决定上限，先于渠道接入。
2. **多 Org 第一公民**：`Org → Project → ApiKey` 三层，永不混淆。
3. **强类型 ID**：编译期防止 `OrgId` 当 `ProjectId` 传。
4. **租户隔离两道闸**：应用层 `WHERE org_id` + 数据库 RLS 兜底。
5. **插件边界先稳后扩**：编译期 provider trait + 运行期 HTTP Plugin manifest 先落地；WASM 先冻结 vNext ABI 设计稿，runtime 等边界稳定后再实现。
6. **预扣 + 修正**：流式扣费三段式，避免热点 + 漏扣。
7. **审计同步落，用量异步落**：区分关键事件与高频事件。

---

## 0.1 文档目标与对齐原则

这份文档不是仓库流水账，而是让新读者在 5 分钟内确认四件事：

1. 这个项目解决什么问题；
2. 运行时怎么分层；
3. 请求如何穿过边界；
4. 哪些文件是 source of truth。

对齐优秀项目的方式，不是堆术语，而是把入口、架构、运行手册和阶段证据分开：

- README：第一屏讲清楚定位、能力、快速启动。
- DESIGN：讲清楚领域模型、运行时边界、关键请求流。
- docs/README：只做文档索引，不承载实现细节。
- docs/stages：只放已经完成的一次性审计 / 收口证据。
- RELEASE / runbook：只写部署、回滚、故障处置。

---

## 0.2 架构总览入口

系统架构图、runtime mode、route boundary、关键请求流和部署形态统一维护在 [docs/architecture.md](./docs/architecture.md)。

`DESIGN.md` 只保留长期设计原则、领域模型、权限、配额、计费、安全与演进边界，避免架构图和实现清单在多处漂移。

---

## 1. 领域模型

```
                       ┌──────────────┐
                       │ Organization │ (顶层租户，计费主体)
                       └──────┬───────┘
                              │ 1:N
              ┌───────────────┼──────────────┐
              ▼               ▼              ▼
        ┌─────────┐    ┌──────────┐   ┌─────────────┐
        │ OrgRole │    │ Project  │   │ Invitation  │
        │ Member  │    └────┬─────┘   └─────────────┘
        └─────────┘         │ 1:N
                            ▼
                    ┌───────────────┐
                    │ ProjectMember │ (M:N → User)
                    │ ApiKey        │
                    │ ModelAlias    │
                    │ Quota[]       │
                    │ AuditLog[]    │
                    └───────────────┘
                            │ M:N
                            ▼
                    ┌───────────────┐         ┌──────────┐
                    │ ChannelGroup  │ ◄─M:N─► │ Channel  │ (平台级)
                    │ (路由策略)    │         │ + KeyPool│
                    └───────────────┘         └──────────┘
```

### 1.1 三层租户的边界

| 层 | 职责 | 关键字段 |
|---|---|---|
| **Org** | 合同/计费/合规主体 | `slug` 全局唯一，`owner_user_id` |
| **Project** | 隔离边界、成本归属、配额单元 | `(org_id, slug)` 唯一 |
| **ApiKey** | 调用凭证 | `project_id` 强绑定，`allowed_models[]` 缩范围 |

**为什么这么分？**
- Org 拆出来是为了未来转 SaaS：一个公司可能有多个事业部，每个事业部独立计费但共享品牌。
- Project 是隔离主体：dev / staging / prod 各一个 Project，配额和成本天然分开。
- ApiKey 强绑 Project：避免 newapi 那种 "key 越权调用其他用户模型" 的雷。

### 1.2 渠道与项目的解耦

**Channel 是平台资源**，由 `platform_admin` 创建——运营运维各管各的。
**ChannelGroup 是路由编排单元**，把多个 Channel 按策略组合（priority / weighted_random / round_robin / least_conn / least_latency / fallback）。
**Project 通过 ProjectGroupBinding 选择能用哪些 Group**——可按 model_pattern 进一步细分。

这层抽象的好处：
- 新增渠道 → 不动 Project 配置
- 切换主备 → 改 Group 内的 priority
- 黑产防控 → 单个 Project 解绑 Group 即停服，不影响其他

---

## 2. RBAC 设计

### 2.1 为什么不用 Casbin/Cedar

- 你的角色组合是**有限的**（Org 4 个 + Project 4 个 + Platform 3 个）
- ABAC 复杂规则用不上，自建 RBAC 编译期映射够快够清晰
- 真到需要时再换，trait 隔离了下层实现

### 2.2 角色矩阵（节选）

| 权限 / 角色 | OrgOwner | OrgAdmin | ProjOwner | ProjAdmin | ProjDev | ProjViewer |
|---|---|---|---|---|---|---|
| `org.update` | ✓ | ✓ | | | | |
| `org.billing.write` | ✓ | | | | | |
| `org.member.invite` | ✓ | ✓ | | | | |
| `project.create` | ✓ | ✓ | | | | |
| `project.delete` | ✓ | ✓ | ✓ | | | |
| `project.member.invite` | ✓ | ✓ | ✓ | ✓ | | |
| `apikey.create` | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `apikey.revoke` | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `quota.write` | ✓ | ✓ | ✓ | ✓ | | |
| `usage.read` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `audit.read` | ✓ | ✓ | ✓ | ✓ | | |

完整映射在 `crates/gate-core/src/rbac.rs`。


### 2.3 用户生命周期管理

平台用户是全局账户，组织与项目成员关系只引用 `users.id`。发布边界要求：

- 创建用户只接受邮箱、展示名、初始密码和状态；密码只在服务端 Argon2id hash，API 与 audit 均不回显明文。
- 平台管理员可停用、启用和重置密码；停用当前登录管理员被拒绝，避免自锁。
- `active` 是唯一可登录/refresh 的状态；`suspended`、`pending_verification`、`deleted` 都不可签发新 token。
- 用户管理 mutation 统一走 `Permission::PlatformAdmin` + `Scope::Platform`，并写入 `user.create` / `user.update_status` / `user.reset_password` 审计事件。

### 2.4 Refresh Session 管理

控制台登录态由 access JWT + refresh JWT 组成，refresh JWT 额外落 `user_sessions` 表：

- `user_sessions.refresh_token_hash` 只存 SHA-256 hash，永不落明文 refresh token。
- `RefreshClaims.sid` 对应 `user_sessions.id`；`jti` 每次轮转生成新值，避免旧 refresh token 重放。
- `/v1/auth/refresh` 必须同时满足 JWT 有效、用户仍为 `active`、session 未撤销/未过期、hash 匹配；成功后原子更新 refresh hash 与 `last_used_at`。
- `/v1/auth/logout` 撤销当前 session，平台管理员可通过 `/v1/admin/users/:id/sessions` 查看用户活跃 session，并用 DELETE 撤销单个或全部 session。
- 撤销 session 只阻断后续 refresh；已签发 access token 继续自然过期。正常 JWT secret 轮换走 `JwtRing` 窗口；需要立即作废 access token 时，应 emergency 切换 primary secret、清空 previous secrets，并撤销相关 session，或后续引入 access-token denylist。

### 2.5 SSO Provider 管理

`identity_providers` 是平台级 / Org 级 OIDC 配置源。当前控制台先开放平台级管理面：

- Admin API：`GET/POST /v1/admin/identity-providers`、`PUT/DELETE /v1/admin/identity-providers/:id`、`POST /discover`，全部要求 `Permission::PlatformAdmin`。
- 公开登录入口：`GET /v1/auth/sso/providers` 只暴露 enabled 平台级 Provider 的 `name/slug`，前端 `/login` 不再硬编码 Google。
- `client_secret` 只在创建或显式轮换时提交；服务端用 `EnvelopeKms::seal` + `aad::idp_secret(provider_id)` 加密，API response 与 audit 均不返回明文 / 密文。
- OIDC discovery 使用 no-redirect HTTP client 验证 `issuer`、`authorization_endpoint`、`token_endpoint`、`jwks_uri`，issuer 不一致直接拒绝。
- `metadata.redirect_policy` 是 SSO redirect 边界：相对路径由 `allow_relative` 控制；绝对 URL 只允许命中 `allowed_origins`，`javascript:`、scheme-relative URL 与未配置 origin 的外站跳转一律拒绝。
- JIT provisioning 仍由 `auto_create_users`、`email_domain_allowlist`、`auto_join_org_role` 三项共同约束；allowlist 不匹配时在创建 / 绑定用户前拒绝。

### 2.6 邀请流

`invitations` 表是 Org / Project 成员邀请的唯一 pending source：

- Admin API：Org 邀请走 `/v1/admin/orgs/:org_id/invitations`，Project 邀请走 `/v1/admin/orgs/:org_id/projects/:project_id/invitations`；创建 / 列表分别要求 `OrgMemberInvite` 或 `ProjectMemberInvite`，撤销要求对应 remove 权限。
- 公开接受入口：`POST /v1/invitations/preview` 只返回目标邮箱、scope、role、状态与过期时间；`POST /v1/invitations/accept` 校验 token pending、邮箱匹配、用户 active 或新建密码用户后写 membership。
- token 明文只在创建响应返回一次，存储层只保存 `SHA-256(token)`；accept 使用 `accepted_at IS NULL AND revoked_at IS NULL AND expires_at > NOW()` 条件更新，天然拒绝重放、过期与撤销。
- Project invite accept 会重新读取 `projects.org_id` 后调用 `add_project_member_in_org`，确保 `AuthContext` 中仍以 `(OrgId, ProjectId)` 复合 key 保存 project role，延续跨 Org 重放防线。
- 关键 mutation 写入 `invitation.create` / `invitation.revoke` audit；accept 当前为公开 self-service 路径，不回显 token hash，也不写明文 token。

### 2.7 SCIM 评估边界

P1.7 只完成 SCIM 2.0 评估，不声明当前已有 SCIM runtime endpoints；长期评估文档见 `docs/scim-evaluation.md`。实现边界如下：

- SCIM 仅作为 Org-scoped inbound provisioning connector，同步企业用户和组，不授予平台级 `PlatformRole`。
- 用户同步以 email 归一化匹配现有 `users.email`，外部稳定键必须落独立 SCIM binding；不得复用 OIDC `user_identities.subject`。
- `active=false` / DELETE 默认映射为 `users.status = suspended` 并撤销 refresh sessions，不硬删用户、不设置密码、不创建 API key。
- SCIM Group 不能直接等同 Kooix role；必须先由管理员配置显式 group → Org/Project role mapping，Project mapping 必须带 Org 上下文并校验 `projects.org_id`。
- SCIM 只能撤销自己授予的 membership grant，不能误删本地手工 Owner / Admin；`owner` mapping 默认禁用，必须显式二次确认。

### 2.8 权限检查门面

所有路由强制走：

```rust
require!(ctx, Permission::ApiKeyCreate, Scope::Project { org, project });
```

不允许在 handler 里手写 if 判断——杜绝散落漏检。

---

## 3. 限流 + 配额模型

### 3.1 分两类，不要混

| 类型 | 性质 | 算法 | 触发后 | 存储 |
|---|---|---|---|---|
| **Rate Limit** | 频率保护 | Redis 滑动窗口 (ZSET + Lua) | 429 | Redis 主存 |
| **Quota / Budget** | 用量配额 | 原子计数 + 周期重置 | 429 | Redis 计数 + PG 持久化 |

### 3.2 多维叠加（取最严）

P1.6 后每个请求依次过：
```
ApiKey × Model → Project × Model → Org × Model
User × Model → Org × Model
```

任一 enabled + `mode=enforce` 规则 deny 即拒。`model_filter` 支持精确值与简单 `*` glob；未命中请求 model 的规则不会参与本次判断。`mode=dry_run` 只记录 would-deny，不扣 Redis、不拦截。

当前维度与计量语义：

| dimension | 计量 | Redis key | reset |
|---|---|---|---|
| `rpm` | request count | `qt:{scope}:{id}:rpm:{model}:{quota_id}` | sliding window |
| `tpm` | estimated tokens | `qt:{scope}:{id}:tpm:{model}:{quota_id}` | sliding window |
| `concurrent` | inflight request units | `qc:{scope}:{id}:concurrent:{model}:{quota_id}` | settle 后 refund |
| `daily_budget_usd` | cost micros | `qb:d:{scope}:{id}:daily_budget_usd:{model}:{quota_id}` | 24h TTL |
| `monthly_budget_usd` | cost micros | `qb:m:{scope}:{id}:monthly_budget_usd:{model}:{quota_id}` | 30d TTL |
| `lifetime_budget_usd` | cost micros | `qb:l:{scope}:{id}:lifetime_budget_usd:{model}:{quota_id}` | never expire |
| `lifetime_tokens` | actual token units | `qtok:l:{scope}:{id}:lifetime_tokens:{model}:{quota_id}` | never expire |

`rpm/tpm` 走 Redis ZSET sliding window；TPM member 前缀保存本次 token amount，兼容旧 member 按 1 计。`concurrent`、budget 与 lifetime 维度走 Redis counter pre-debit，handler 成功后按实际 cost/tokens settle，多退少补。

### 3.3 流式扣费三段式

```
PRE  (请求接入):
  estimated = max_tokens × output_price
  redis.eval('check_and_deduct.lua', quota_keys, estimated)
  if denied → 429/402
  insert into inflight_requests

STREAM (流式中):
  仅累加展示用 token 计数，不动配额（避免热点）

POST (流结束 / 错误):
  真实 usage 回来
  redis.eval('adjust.lua', quota_keys, real - estimated)
  delete from inflight_requests
  enqueue outbox_events('usage', record)
```

后台 worker 消费 outbox → 批量写 `request_events` / `usage_records` projection、批量 upsert hourly/daily rollups、批量写 `billing_ledger_events.actual_settle`，再批量 `mark_done`；duplicate `idempotency_key` 只记一次账但会把重复 outbox row 标为完成，触发告警、对账。

**关键：超时回滚**。`inflight_requests.expires_at` 设 `max_tokens / 10 tps` 的预估时间，cleaner 定时跑回滚。

当前实现把 counter 类 quota 的预扣状态同时写入 Redis 与 `inflight_requests`：

- Redis key 由 `scope_kind/scope_id/dimension/model_filter/quota_id` 组成，避免多条 quota 串桶。
- `inflight_requests.quota_keys` / `estimated_micros` 记录本请求所有成功预扣项；字段名 `estimated_micros` 是历史兼容名，P1.6 后可表示 cost micros、token units 或 concurrent unit，真实语义由内存 guard 的 `QuotaMetric` 区分。
- 正常路径由 `InflightGuard::settle_units(actual_units)` 多退少补并删除 inflight 行。
- handler panic / 取消时 `Drop` 全额退还；进程崩溃时后台 sweeper 每 60s 删除过期 inflight 行并按 `quota_keys × estimated_micros` 退还 Redis。

### 3.4 Quota policy diagnostics

控制面新增两条诊断链路：

- `GET /v1/orgs/:org_id/quotas/explain`：按 scope/model/估算 tokens/cost 解释会命中的规则，返回 `current_used`、`estimated`、`remaining`、`would_deny`、`retry_after_ms` 与 `reset_at`。
- `GET /v1/orgs/:org_id/quotas/reconcile`：对 counter 维度读取 Redis，对 budget / lifetime tokens 从 `usage_records` 聚合 PG projection，返回 `redis_used`、`pg_used`、`delta` 与 runtime-only / glob best-effort 说明。

`quota_dry_run_total{dimension,scope_kind,would_deny}` 是 dry-run 策略的主信号；dry-run 规则只 `peek` 当前 Redis 用量，失败按 fail-open 处理并写 warning。

### 3.5 Billing ledger 与 invoice 状态机

P1.5 后 `billing_ledger_events` 是计费审计源，`usage_records` 只作为控制台 / analytics projection：

- `estimated_debit`：预算 / quota pre-debit 预扣。
- `actual_settle`：请求完成后的实际扣费；月账单费用优先从此事件重建。
- `refund` / `manual_adjustment`：退款与人工调账。
- `invoice_close`：月账单关闭快照 marker。

月账单 operational state 存在 `billing_invoices`，严格前进：`draft -> closed -> exported -> paid/waived`。`exported` 必须绑定导出 digest（CSV 响应头或 JSON payload 的 `sha256:<hex>`），状态推进写 `billing.invoice.transition` audit。

---

## 4. Channel & Key 池

### 4.1 健康熔断

每个 `channel_keys.health` 状态机：

```
healthy ──5次连续错误──→ cooling_down (60s)
                            │
                            ├──冷却后探活成功──→ healthy
                            └──探活失败──→ cooling_down (×2 指数退避)

任意状态 ──手动──→ disabled
```

Channel 级 `health` 由所有 key 状态聚合：
- 全部 healthy → healthy
- 部分 cooling_down → degraded
- 全部 cooling_down/disabled → unhealthy（触发 Group fallback）

### 4.2 路由策略

| Strategy | 选择算法 |
|---|---|
| `priority` | 按 priority 升序，第一个 healthy 的 channel |
| `weighted_random` | 按 weight 加权随机（健康 key 中） |
| `round_robin` | 轮询（健康 key 中） |
| `least_conn` | 选择当前 inflight 最少的 channel，完成后释放计数 |
| `least_latency` | 选择最近延迟最低的 channel；无指标时回退到 priority 顺序 |
| `fallback` | 当前 group 无可用 channel 或 disabled 时切 `fallback_group_id`，最大深度 5 防环 |

### 4.3 HTTP Plugin 渠道与 SSE 整流

`provider_type=plugin|custom|http|http_plugin` 走运行时 HTTP plugin adapter，不需要重新编译 provider crate。插件 manifest 存在 `channels.model_mapping.plugin`（复用已暴露 JSONB 配置面，密钥仍走 `channel_keys` / env 回退）：

- `preset.provider`：主流 Provider 预设，当前覆盖 `openai`、`openai_compatible`、`anthropic_messages`、`azure_openai`、`vertex_openai`、`gemini`、`deepseek`、`mistral`、`cohere_chat`、`ollama`、`groq`、`together`、`openrouter`、`moonshot`、`zhipu`、`qwen`、`yi`、`bedrock_converse`；预设负责默认 path、headers、request adapter、response/SSE mapper。
- `request.chat_path`：默认必须是相对 `base_url` 的 path，支持模板变量；Azure 预设用 `{{model}}` 展开 deployment path。绝对 URL 需要显式 `security.allow_absolute_chat_path=true` 与 `security.permissions.absolute_urls=true`，且仍会拒绝 localhost、link-local、private IP、metadata host 与 DNS rebind。
- `request.headers`：支持 `{{api_key}}` 等模板变量，未声明 Authorization 时默认 Bearer；设为 `null` 可显式禁用默认 Bearer。
- `request.body`：JSON 模板，支持 `{{model}}`、`{{messages}}`、`{{last_user_message}}`、`{{stream}}`、`{{max_tokens}}` 等变量；整段占位会保留原 JSON 类型。
- `security.*`：限制 request body、response body、SSE event 大小与 request timeout；`outbound_allowlist` 强制 origin allowlist；header/path/body 模板分域白名单校验，manifest 作为不可信配置处理。
- `response.*_path`：把私有非流式响应抽成 `ChatResponse`。
- `stream.*_path`：共享 SSE decoder 先处理 CRLF/LF、注释、多行 data、分片，再按 path 抽 token / finish_reason / usage，归一成 `ChatStreamChunk`；OpenAI-compatible 预设自动注入 `stream_options.include_usage=true`。

这条链覆盖 OpenAI-compatible、Anthropic Messages、Azure deployment URL、Vertex AI OpenAI endpoint、包装型私有 JSON、纯 token SSE、`data: EOF` 等奇葩格式；manifest v1 边界与示例见 `docs/plugin-manifest.md`。WASM runtime 仍延后，vNext ABI 设计稿见 `docs/wasm-plugin-abi.md`，避免早期把执行沙箱冻结成生产承诺。

### 4.4 Typed ID API 边界

数据库层继续使用裸 UUID，领域类型在 API 边界负责展示与兼容：

- `OrgId` / `UserId` / `ProjectId` / `ChannelId` 等 `Display` / `Serialize` 输出 `{prefix}_{uuid_simple}`，例如 `org_019e2c1ba7d17162842207e4b24f5f98`。
- `FromStr` / `Deserialize` 同时接受 typed ID 与裸 UUID，便于灰度迁移和内部调用。
- URL path extractor 使用 `FlexUuid`，所有 `/:id` 路由可接收 `ch_...` / `usr_...` 或 `019e...` 裸 UUID。
- 前端通过 `web/src/lib/id.ts` 的 `rawId()` 把 typed ID 转回路径用 UUID，`shortId()` 用于表格短显。

---

## 5. 数据库决策

### 5.1 PostgreSQL 单实例 vs 集群

- 3000 用户、5w rpm 的写入主要在 `request_events` / `request_log_events` / `usage_records`（月级十亿行）
- **配置类表**（users / projects / channels …）QPS 很低，单实例几十年都跑得动
- v0.2.0 migration 默认在普通 PostgreSQL 15+ 上运行；TimescaleDB 不作为启动硬依赖。
- `request_events` 保留 canonical idempotency / settlement 语义；`request_log_events` 是按月 range partition 的请求日志 read projection，由 `request_events` insert trigger 自动投影，列表 / 筛选 / incident summary 优先读该分区表。
- `kooix_ensure_request_log_partitions(months_ahead)` 预建当前 + 未来分区；`kooix_prune_request_log_partitions(retention_months, dry_run)` 与 `kooix_prune_request_log_details(retention_days)` 提供可审计 retention。
- 生产高吞吐（5w rpm 级别）可选 TimescaleDB profile：`request_events` / `request_log_events` / `usage_records` 按时间转 hypertable，配合 compression + retention；普通 PostgreSQL 路径默认可运行。

### 5.2 Row-Level Security 兜底

```sql
SET LOCAL app.current_org_id = '<uuid>';
SET LOCAL app.is_platform_admin = 'false';
```

应用层每个请求开始时设置，所有 `project` 范围表自动 `WHERE org_id = current_org_id()`。

应用层漏写 filter 也不会泄露——多一道保险。

### 5.3 强类型 ID

`gate-core::id` 用宏生成 `OrgId/UserId/ProjectId/...`，编译期防止串台。`Display` 给前缀（`org_xxx`, `proj_xxx`），日志友好。

### 5.4 Pricing rules 管理面

`pricing_rules` 是当前定价主表，支持 global 与 channel-specific 两级规则：

- `dimension × unit × conditions JSON` 描述多模态计费维度，`priority` 控制同维度匹配顺序。
- `channel_id = NULL` 表示全局规则；非空时覆盖特定 channel。
- REST 管理面：`GET/POST /v1/admin/pricing-rules`、`DELETE /v1/admin/pricing-rules/:id`，全部要求 `Permission::PlatformAdmin`。
- CLI 管理面：`kgctl pricing list|set|delete`，用于无控制台或运维脚本场景。
- 控制台页面：`/admin/pricing` 复用 DataToolbar / DataTable 模板，支持按模型和渠道过滤。

---

## 6. 关键决策记录（ADR-style）

| # | 决策 | 理由 |
|---|---|---|
| 1 | 多 Org 设计直接落 | 单 Org 后期加 Org 列代价极高，先做不亏 |
| 2 | sqlx 不用 ORM | 编译期校验 SQL，ORM 的抽象在这类场景反受其累 |
| 3 | RLS 兜底 | 应用层 bug 不可避免，DB 多一道隔离 |
| 4 | Channel 平台级 | 运营与租户解耦，符合 SaaS 心智 |
| 5 | 流式三段式扣费 | 实时扣 → 热点；事后扣 → 漏扣；预扣 + 修正是唯一解 |
| 6 | TimescaleDB 作为高吞吐可选增强 | v0.2.0 默认普通 PostgreSQL 可跑；5w rpm × 30 天 = 21 亿行时建议 hypertable |
| 7 | WASM runtime 延后，先落 HTTP Plugin manifest + Provider 预设；ABI 只做 vNext 设计稿 | trait 抽象先稳定，主流 Provider 与私有协议主要差异可先用 preset、request/response/SSE path 映射吸收；WASM 执行面必须先明确 transform、secret、determinism、resource audit 边界 |
| 8 | 自建 RBAC | 角色组合有限，Casbin 引入心智税不值 |
| 9 | API 对外 typed ID，DB 继续裸 UUID | 外部可读性和防串台更强，存储/索引/外键不迁移，`FlexUuid` 保持向后兼容 |
| 10 | Pricing rules 暴露 REST + CLI + UI 三入口 | 运营日常用 UI，部署/批量改价用 CLI，自动化接 REST；三者复用同一 `pricing_rules` 主表 |

---

## 7. 安全运维注记（必读）

### 7.1 Master Key 备份策略

`KOOIX_MASTER_KEY` 是 envelope encryption 的 KEK，**丢失等于所有渠道密钥/OIDC client_secret 全部失效**。

部署流程：

```bash
# 1. 生成密钥（一次性）
cargo run -p kgctl -- init > deploy/secrets.env

# 2. 立刻分发三处保存
#    a. 密码管理器（1Password / Bitwarden 团队保险柜）
#    b. 云 KMS / Vault Transit
#    c. 物理离线备份（U 盘 + 保险箱）

# 3. 部署后用 echo $KOOIX_MASTER_KEY 验证已注入
# 4. secrets.env 立即从磁盘清除：shred -u deploy/secrets.env
```

**轮换策略**：master key 轮换需要重新加密所有 `*_enc` 列。当前未实现轮换工具，路线图上 P1。

### 7.2 AAD 绑定约定（防密文移植）

所有加密字段**必须用 `gate_crypto::aad` 模块的助手生成 AAD**，杜绝凭空写约定字符串：

| 加密字段 | AAD 助手 | 防御目标 |
|---|---|---|
| `channels.config_enc` | `aad::channel_config(channel_id)` | DBA 把 A 渠道的配置挂到 B 渠道复用 |
| `channel_keys.key_enc` | `aad::channel_key(channel_id)` | 把 A 渠道的 key 密文搬到 B 渠道复用 |
| `identity_providers.client_secret_enc` | `aad::idp_secret(provider_id)` | 把测试 IdP 的 secret 嫁接到生产 IdP |

写入 / 读取必须用同一个 AAD。AAD 不需保密，写错时 AEAD 验证会直接 `AeadFailed`——肉眼可见的报错好过静默泄露。

### 7.3 JWT Secret 纪律

- `JwtIssuer` 是 `JwtRing` 的兼容别名：只用 primary secret 签发，按 primary → previous 顺序验签 access / refresh。
- `JwtIssuer::new` / `with_previous_secret` 强制 secret >= 32B（短了直接 `AuthError::Invalid`）
- 生产用 64B：`cargo run -p kgctl -- key jwt`
- `KOOIX_JWT_SECRET` 是 primary signing key；新 access / refresh token 永远只用它签发。
- `KOOIX_JWT_PREVIOUS_SECRETS` 是可选旧 key 窗口，逗号分隔 base64 secret；只用于验签，不签发新 token。
- 正常轮换：新 key 写入 `KOOIX_JWT_SECRET`，旧 key 临时移入 `KOOIX_JWT_PREVIOUS_SECRETS`，等待最长 access / refresh TTL 或运营窗口后移除旧 key。
- 泄露处置：不要把泄露 key 放入 previous；直接替换 primary、清空 previous 并撤销 session，让旧 token 全部失效。

### 7.4 require! 纪律

**所有 handler 必须以 `require!(ctx, ...)` 开头**。Code review 的 grep 命令：

```bash
# 查找可疑的「绕过授权」写法
rg 'fn.*\(.*AuthContext' --type rust -A 5 | rg -v 'require!|can!|require_user!|require_api_key!'
```

`AuthContext` 内部 `org_memberships` / `project_memberships` / `platform_role` 字段已收为 `pub(crate)`——外部 crate 拿不到 raw 映射，**只能**通过 `can()` / `require()` / 只读 `*_role()` accessor 访问。

`project_memberships` 用复合 key `(OrgId, ProjectId)`——攻击者拿到合法 project_id 替换 Org 上下文重放无效（被 `cross_org_project_id_replay_denied` 测试覆盖）。

### 7.5 部署 env 清单

完整清单见 `cargo run -p kgctl -- env`。最小集：

```
KOOIX_MASTER_KEY    # base64 32B (kgctl key master)
KOOIX_JWT_SECRET    # base64 64B (kgctl key jwt)
KOOIX_JWT_PREVIOUS_SECRETS # optional comma-separated old JWT secrets for rotation verify window
KOOIX_DATABASE_URL  # postgres://...
KOOIX_REDIS_URL     # redis://...
KOOIX_PUBLIC_URL    # https://gate.example.com — OIDC redirect_uri 基底
KOOIX_CHANNEL_KEY_CACHE_TTL_SECS # optional，channel key 解密缓存 TTL，默认 30s，0 禁用
```

---

## 8. 待办路线

- [x] Workspace + 领域类型 + 完整 schema
- [x] gate-crypto envelope encryption + AAD 类型化助手
- [x] gate-auth: password / JWT / API key / OIDC / AuthContext / require!
- [x] kgctl: 部署密钥生成 + env 清单
- [x] SSO schema (identity_providers + user_identities + oidc_login_states)
- [x] gate-storage Repo 实现（users / orgs / projects / memberships / api_keys / channels / quotas / inflight）
- [x] gate-cache: Redis 滑动窗口 Lua + 配额扣减 Lua
- [x] gate-server: Axum AppState + Auth 抽取器 + 控制台/API 路由
- [x] gate-providers: 9 个编译期 Provider + HTTP Plugin adapter + Provider preset
- [x] gate-billing: usage outbox 批量消费者 + pricing rules + LiteLLM sync
- [x] web: SvelteKit 控制台
- [x] HTTP Plugin manifest v1 schema / builder / replay debugger
- [x] WASM 插件 ABI vNext 设计稿
- [ ] WASM sandbox runtime
- [ ] master key 轮换工具
- [x] `JwtRing` 双密钥轮换窗口
