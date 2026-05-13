# Changelog

All notable changes to **Kooix Gate** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.0] — 2026-05-14

第一个里程碑。覆盖从领域建模到控制台的完整闭环，单机生产可用的最小集。

**Workspace**: 8 crates · 11+ migrations · 170 tests (all green) · `clippy -D warnings` 通过

### Crates

| Crate | 职责 |
|---|---|
| `gate-core` | 强类型 ID / Identity / RBAC / Quota domain types |
| `gate-crypto` | Envelope encryption + KMS 抽象 |
| `gate-storage` | PostgreSQL Repository（trait + Pg + InMemory 双实现） |
| `gate-auth` | Argon2 password / JWT / API Key / OIDC / AuthContext |
| `gate-cache` | Redis Lua: sliding window + quota debit/refund |
| `gate-providers` | OpenAI-compatible Provider trait + ProviderRouter |
| `gate-billing` | Outbox UsageEvent + Pricing + cost_micros |
| `gate-server` | Axum HTTP 网关（auth / chat / sso / usage / quota / channels） |
| `kgctl` | 部署运维 CLI |

### Added — 多租户基座

- 三层租户模型：Org → Project → ApiKey，永不耦合
- 强类型 ID（`OrgId`/`ProjectId`/`ApiKeyId` 等），编译期阻止串台
- RBAC：Platform / Org / Project 三套 Role，复合权限点
- `AuthContext::can()` / `require()` 统一权限门面，禁止外部直接读角色映射
- `require!` / `require_user!` macro 编译期对齐授权检查
- 11 个 SQL migration（含 RLS policies）

### Added — 鉴权

- Argon2id 密码（自适应 cost）+ 5 次失败锁定 (`/v1/auth/login`)
- JWT access (15min) + refresh (30d) 分 audience 隔离
- API Key (`sk-kg-*`) SHA-256 hash + constant-time 比较 + CIDR allowlist
- OIDC/SSO：discovery → PKCE → JIT provisioning → user_identities 绑定
  - 平台级 IdP (`identity_providers.org_id IS NULL`) + Org 级 SSO
  - 邮箱域白名单 + auto_join_org_role
- `AuthContextLoader` trait（`InMemoryLoader` / `PgLoader` 双实现，dev/prod 切换）
- `Authed` / `MaybeAuthed` extractor + `X-Kooix-Org` 切换租户 + 越权防护

### Added — 路由 & 计费

- `/v1/chat/completions` OpenAI 兼容（流式 SSE + 非流式）
- `ProviderRouter` 按 `project_id + model` 选 channel（priority 策略）
  - 返回 `RoutedProvider { provider, channel_id }`，channel_id 透传到计费
- `X-Kooix-Project` header 让 User 主体也能命中 channel 路由（含越权校验）
- 流式 token 计费：强制注入 `stream_options.include_usage`；流尾捕获 usage 后 `tokio::spawn` 推 outbox，不阻塞客户端
- Outbox pattern：`UsageEvent → outbox_events → Consumer → usage_records`（幂等 `ON CONFLICT DO NOTHING on (ts, request_id)`）
- `ModelPricing` 查询 + `compute_cost_micros`（channel 优先 → 全局 fallback）

### Added — 限流 & 配额

- 全局限流 middleware（subject-bucketed，挂在 `/v1/*`，fail-open）
  - 优先级：ApiKey > User > X-Forwarded-For IP
  - 健康检查 `/health` 不受限流影响
- Quota middleware（rate / budget 双维度）
  - rpm / tpm / daily_budget_usd / monthly_budget_usd / lifetime_tokens
  - Redis Lua：sliding window ZSET + 原子预扣/退还
  - `peek` 模式：chat 路径只读判断已超额，不预扣
- `/v1/orgs/:org/quotas` CRUD（含跨 Org 越权写校验）

### Added — 控制台

- `/v1/usage` 聚合 endpoint（`?range=7d|30d&group_by=day|model|channel`）
  - SuperAdmin 可跨 Org，普通用户锁定 `current_org`
- `/v1/orgs/:org/channels` 只读 admin 视图（不返回密钥）
- SvelteKit + TypeScript + Tailwind 控制台
  - `/login` 密码 + Google SSO 双入口
  - `/usage` 三 stat cards + 零依赖 SVG 折线图（7d/30d 切换）
  - `/channels` 状态/健康度 badge 表格
  - `/orgs` + `/orgs/[orgId]/projects` 列表 + inline 创建
  - 共享 NavBar，未登录自动跳 `/login`

### Added — 运维

- `kgctl migrate [--dry-run]`：sqlx migration runner
- `kgctl admin create --email [--password]`：写 users + platform_admins(super_admin)，幂等保护
- `kgctl doctor`：MASTER_KEY / JWT_SECRET / DB / Redis 四项 preflight
- `kgctl seed-pricing`：5 个主流模型默认 USD/M token 价格（幂等）
- `kgctl init / key / env`：首次部署密钥生成 + env 清单

### Tests

- 170 测试全绿（unit + integration）
- testcontainers 17-alpine（`KOOIX_TEST_PG_TAG` env override）
- wiremock 假装上游 OpenAI / OIDC IdP
- InMemory repo 与 Pg repo 跑同一份契约测试

### Security

- 跨 Org 重放防护：project_memberships 用 `(OrgId, ProjectId)` 复合 key
- SuperAdmin 短路所有权限检查（且仅平台级 token 能拿到）
- API key 撤销立即生效（每次请求查 `revoked_at`）
- OIDC state 一次性消费（`DELETE ... RETURNING`，state 仅存 SHA-256）
- client_secret 全程密文，envelope encryption + AAD 绑定 provider_id

### Known limitations

- D5 待办：`inflight_requests` 表已建但流式预扣还没接 chat handler
- 多 provider 翻译：当前只有 OpenAI 兼容，Anthropic / Gemini 待补
- WASM 插件延后：trait 抽象稳定后再开 ABI

[0.1.0]: https://github.com/telagod/kooix-gate/releases/tag/v0.1.0
