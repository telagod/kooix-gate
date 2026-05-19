<p align="center">
  <img src="./web/src/lib/assets/kooix-logo.svg" alt="Kooix 空衍 logo" width="128" height="128" />
</p>

# Kooix Gate · 空衍

> Rust + Svelte 打造的 LLM 网关。多 Org 三层租户、9 Provider 多模态、HTTP Plugin 私有协议接入、流式正确计费、可视化编排、配额拦截、SSO/OIDC。
>
> **空衍**：以四向 super-star 为核，以 D4 旋转轨道为门；在对称星图中收束私有协议、认证、SSE 与字段映射。

竞品定位：NewAPI / OneAPI / LiteLLM 的「底盘加强版」——把它们反复踩的雷（权限粗、限流单一、租户隔离漏、流式漏扣）先治好，再谈渠道接入。

[![Tests](https://img.shields.io/badge/tests-277%20Rust%20%2B%2055%20web-brightgreen)](#测试)
[![Rust](https://img.shields.io/badge/rust-2024-orange)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue)](./LICENSE)

## 当前版本：v0.2.0

详见 [CHANGELOG.md](./CHANGELOG.md)。

> v0.2.0 是第一个正式发布版本：把 typed ID、pricing rules、crash-safe quota pre-debit、HTTP Plugin SSE normalizer、Provider 插件预设、发布 runbook 与渠道插件化 roadmap 一起收口。

### 核心能力

- ✅ 多 Org × Project × ApiKey 三层租户 + RBAC + RLS 兜底
- ✅ 9 Provider 适配（OpenAI / Anthropic / Azure / Gemini / DeepSeek / Mistral / Groq / Moonshot / Bedrock）
- ✅ HTTP Plugin 渠道：用 JSON manifest 接入私有协议、奇葩 body/response、非标准 SSE token 帧
- ✅ Plugin 安全护栏：manifest 模板白名单、绝对 URL 默认禁用、内网/metadata host 拒绝、body/response/SSE size limit
- ✅ Provider 插件预设：OpenAI-compatible / Anthropic / Azure / Gemini / DeepSeek / Mistral / Cohere / Ollama / Bedrock 等主流渠道可统一按 plugin manifest 接入
- ✅ `/v1/chat/completions` OpenAI 兼容（流式 SSE + 非流式 + tool calling）
- ✅ typed ID API response（`org_...` / `proj_...` / `usr_...`），路径参数仍兼容裸 UUID
- ✅ 5 种路由策略（priority / weighted_random / round_robin / least_conn / least_latency）
- ✅ 多维度计费引擎（token / image / audio / cache / batch，自动同步 LiteLLM 定价，ledger 对账 + invoice 状态机）
- ✅ Quota policy engine（rpm / tpm / concurrent / daily / monthly / lifetime，Redis Lua 原子，dry-run / explain / reconcile）
- ✅ Refresh session 管理：refresh token hash 持久化、轮转、防重放、logout 撤销、平台管理员踢下线
- ✅ 可视化编排 Playground（@xyflow/svelte 节点式流程编辑器）
- ✅ SvelteKit 控制台（channel 管理 / 请求日志 / usage 仪表盘 / 月度账单 / SSO）
- ✅ `kgctl` 部署 CLI + Docker Compose 一键部署 + GitHub Actions CI

### v0.2.0 新增亮点

- 🧩 typed ID 输出与 `FlexUuid` 路径参数兼容，前端用 `rawId()` / `shortId()` 处理展示和跳转。
- 💸 Pricing rules 管理闭环：`/v1/admin/pricing-rules`、`kgctl pricing list|set|delete`、`/admin/pricing` 控制台页面。
- 🧯 Crash-safe quota pre-debit：`inflight_requests.quota_keys/estimated_micros` + 60s sweeper 自动退还过期预扣；P1.6 扩展到 concurrent、lifetime budget、lifetime tokens 与 dry-run policy。
- 🌊 HTTP Plugin SSE normalizer + Provider preset，覆盖私有 SSE 帧、Anthropic Messages、Azure deployment URL 与 OpenAI-compatible usage 末帧。
- 🛡️ HTTP Plugin manifest 作为不可信配置处理：模板变量分域白名单、绝对 URL 默认禁用、内网/metadata host 拒绝、request/response/SSE size limit。
- 🧱 前端模板化：`PageShell` / `SectionCard` / `DataToolbar` / `DataTable` 等集中到 `web/src/lib/components/templates/`。
- 📜 发布收口：`ROADMAP.md`、`RELEASE.md`、`docs/README.md`、`docs/plugin-manifest.md`、`docs/security-runbook.md`、`examples/`。

## 技术栈

| 层 | 选型 |
|---|---|
| Backend | Rust 2024 · Axum 0.7 · Tokio · sqlx 0.8 |
| Storage | PostgreSQL 15+（可选 TimescaleDB）· Redis (fred) |
| Frontend | SvelteKit 2 · Svelte 5 · TypeScript · Tailwind v4 · @xyflow/svelte |
| Auth | Argon2id + JWT (HS256) + API Key (SHA-256) + OIDC |
| Crypto | AES-256-GCM envelope encryption + KMS 抽象 |
| Observability | tracing + OpenTelemetry + Prometheus |

## Workspace 结构

```
kooix-gate/
├── Cargo.toml                  # workspace
├── CHANGELOG.md
├── DESIGN.md
├── LICENSE                     # AGPL-3.0
├── ROADMAP.md
├── RELEASE.md
├── docs/                       # 文档索引 / runbooks / waivers / stages
├── crates/
│   ├── gate-core/              # 领域类型（强类型 ID / Identity / RBAC / Quota）
│   ├── gate-crypto/            # Envelope encryption + KMS 抽象
│   ├── gate-storage/           # PostgreSQL Repository (Pg + InMemory)
│   │   └── migrations/         # 25 SQL 文件，含 RLS / pricing_rules / inflight recovery
│   ├── gate-auth/              # Password / JWT / API Key / OIDC / AuthContext
│   ├── gate-cache/             # Redis Lua（rate limit + quota）
│   ├── gate-providers/         # Provider trait + 9 adapters + ProviderRouter
│   ├── gate-billing/           # Outbox + Multi-dimensional Pricing + LiteLLM sync
│   ├── gate-server/            # Axum HTTP 网关（主二进制）
│   └── kgctl/                  # 部署运维 CLI
└── web/                        # SvelteKit 控制台
```

## Quick Start (Docker)

```bash
# 一键部署（构建镜像 + 起 PG / Redis / 迁移 / 服务）
docker compose up -d

# 仅起基础设施（本地开发用，自己编译运行后端）
docker compose -f docker-compose.dev.yml up -d
cargo run -p gate-server
```

服务启动后访问 `http://localhost:8000`。

> **生产部署**：务必替换 `docker-compose.yml` 中的 `KOOIX_JWT_SECRET`、`KOOIX_MASTER_KEY` 和 `POSTGRES_PASSWORD`。
> 可用 `kgctl init` 生成安全密钥。

## 快速开始（手动）

### 1. 起依赖

```bash
docker run -d --name kg-pg -e POSTGRES_PASSWORD=devpass \
  -e POSTGRES_DB=kooix_gate -p 5432:5432 postgres:17-alpine
docker run -d --name kg-redis -p 6379:6379 redis:7-alpine
```

### 2. 生成密钥 + 跑迁移

```bash
cargo install --path crates/kgctl

# 生成 master key + JWT secret
kgctl init > .env
source .env

export KOOIX_DATABASE_URL=postgres://postgres:devpass@localhost/kooix_gate
export KOOIX_REDIS_URL=redis://localhost:6379/0
export KOOIX_PUBLIC_URL=http://localhost:8080

kgctl migrate
kgctl doctor    # 校验 env / migration / Redis Lua 全绿
kgctl seed-pricing
kgctl admin create --email you@example.com
```

### 3. 起服务

```bash
# 后端
cargo run -p gate-server --release

# 控制台
cd web && npm install && npm run dev
```

### 4. 调一发 chat

```bash
# 用 admin 登录拿 token，从控制台建 API key
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer sk-kg-..." \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"hi"}]}'
```

### 用户管理发布边界

平台管理员控制台提供 `/admin/users` 用户生命周期管理：

- `GET /v1/admin/users?limit=&offset=`：分页列出平台用户，返回 typed `usr_...` ID，不包含 `password_hash`。
- `POST /v1/admin/users`：创建密码用户，字段为 `email`、`display_name?`、`password`、`status?`。密码由后端使用 Argon2id hash 后存储，明文不回显、不写 audit。
- `PUT /v1/admin/users/:id/status`：平台管理员切换 `active` / `suspended` / `pending_verification`，拒绝停用当前登录管理员，避免自锁。
- `PUT /v1/admin/users/:id/password`：平台管理员重置用户密码并清零失败登录计数。
- `GET /v1/admin/users/:id/sessions`：查看该用户仍可 refresh 的活跃 session，不返回 refresh token hash。
- `DELETE /v1/admin/users/:id/sessions/:session_id`：撤销单个 session；`DELETE /v1/admin/users/:id/sessions` 批量撤销，阻断后续 refresh。

安全边界：所有 `/v1/admin/users*` mutation 必须通过 `Permission::PlatformAdmin`；`login` 与 `refresh` 都会检查用户当前状态，非 `active` 用户无法获得新 token。refresh token 只以 SHA-256 hash 落 `user_sessions`，每次 refresh 成功都会轮转并拒绝旧 token 重放。关键 mutation 写入 audit action：`user.create`、`user.update_status`、`user.reset_password`、`user_session.revoke`、`user_session.revoke_all`。

## 设计要点速览

- **多 Org 三层租户**：Org → Project → ApiKey，永不耦合
- **project_memberships 用 `(OrgId, ProjectId)` 复合 key**：防止跨 Org 重放
- **RLS 兜底**：应用层漏 filter 也泄露不了租户数据
- **流式计费正确性**：`stream_options.include_usage` 强制注入，末帧捕获 usage 后写 outbox；worker 落 `usage_records` projection + `billing_ledger_events.actual_settle`
- **Outbox pattern**：业务事务和计费写入解耦，幂等 `ON CONFLICT DO NOTHING`
- **Billing ledger / invoice**：`billing_ledger_events` 是审计源；月账单状态机 `draft -> closed -> exported -> paid/waived`，导出 digest 绑定审计留存
- **Channel 平台级 + Group 编排**：运营和租户解耦，channel_keys envelope encrypted
- **HTTP Plugin 整流**：`provider_type=plugin` 时 `model_mapping.plugin` 作为 manifest，声明 request body/header、非流式 response path、流式 SSE event/token/usage path，统一归一为 OpenAI-compatible `ChatResponse` / `ChatStreamChunk`；manifest 按不可信配置处理，默认不允许绝对 URL，模板变量与 body/response/SSE size 均有限制
- **Provider 插件预设**：`model_mapping.plugin.preset.provider` 可选 `openai`、`openai_compatible`、`anthropic_messages`、`azure_openai`、`gemini`、`deepseek`、`mistral`、`cohere_chat`、`ollama`、`groq`、`together`、`openrouter`、`moonshot`、`zhipu`、`qwen`、`yi`、`bedrock_converse` 等，把主流 Provider 也收敛到同一 plugin manifest 接入面
- **强类型 ID**：编译期阻止 `OrgId` 当 `UserId` 传
- **typed ID 边界**：API response 序列化为 `{prefix}_{uuid_simple}`；route extractor 通过 `FlexUuid` 同时接收 typed ID 和裸 UUID，数据库仍存裸 UUID
- **AuthContext 单一权限门面**：禁止外部读 raw 角色映射，全走 `can()` / `require!`
- **多维度计费**：按 dimension × conditions 精准匹配，支持缓存折扣、批量折扣、分层定价
- **crash-safe pre-debit**：budget quota 先 Redis 预扣，再把 `quota_keys` / `estimated_micros` 写入 `inflight_requests`；正常 settle 多退少补，异常 drop 全退，进程崩溃由 sweeper 兜底

详细架构见 [DESIGN.md](./DESIGN.md)，HTTP Plugin manifest 示例见 [docs/plugin-manifest.md](./docs/plugin-manifest.md)。文档总入口见 [docs/README.md](./docs/README.md)，已完成的阶段性审计与收口记录统一放在 [docs/stages/](./docs/stages/)。SDK / curl / Postman / Bruno / OpenAPI / Terraform / Helm 示例见 [examples/](./examples/)。

## 测试

```bash
# 全量（当前 277 Rust test list entries：272 unit/integration + 5 doctest，含 testcontainers 集成测试，需要 Docker）
cargo test --workspace

# 仅快速 unit（无 Docker）
cargo test --workspace --lib

# 跳过 PG 集成测试
KOOIX_SKIP_PG_TESTS=1 cargo test --workspace

# 覆盖率门禁
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

控制台构建：

```bash
cd web && npm run check && npm test && npm run build
```

## License

[AGPL-3.0-only](./LICENSE)
