<p align="center">
  <img src="./web/src/lib/assets/kooix-logo.svg" alt="Kooix 空衍 logo" width="128" height="128" />
</p>

# Kooix Gate · 空衍

[English README](./README.md)

**面向多租户运营的 Rust LLM 网关——把 OneAPI 反复踩的坑先治好，再谈渠道接入。**

写一份 JSON manifest 就能上一个新渠道，不发版；流式计费 fail-closed；多 Org RLS 兜底；编译期 SQL；强类型 ID。

[![Tests](https://img.shields.io/badge/tests-556%2B%20Rust%20%2B%20127%20web-brightgreen)](#测试)
[![Rust](https://img.shields.io/badge/rust-2024-orange)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/version-0.5.0--rc2-blue)](./CHANGELOG.md)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue)](./LICENSE)

## 是什么

| 维度 | 答案 |
|------|------|
| **定位** | 多租户 LLM 网关 + 控制台。Rust 后端 + Svelte 前端，单 binary 17 MB |
| **核心场景** | 公司/团队对外发 API key，对内统一接 N 家上游，结算到月账单，配额拦截，审计齐全 |
| **不是什么** | 不是 LLM 路由 SDK（用 LiteLLM）；不是无状态代理（用 Cloudflare AI Gateway）；不是单租户 chat UI（用 LobeChat）|

## 跟谁不同

| | Kooix Gate | LiteLLM | OneAPI / NewAPI | OpenRouter |
|---|---|---|---|---|
| **多 Org 租户** | ✅ 三层 + RLS 兜底 | ❌ 单租户 | ⚠ 用户级 | ☁ SaaS |
| **流式计费** | ✅ fail-closed + outbox | ⚠ 有跳过窗口 | ⚠ 静默漏扣 | ☁ SaaS |
| **私有协议接入** | ✅ JSON manifest（5 分钟）| ✅ Python config | ⚠ 改 Go 代码 | ❌ |
| **WASM Plugin** | ✅ ADR-0003 v0 transform hook（0.4.x） | ❌ | ❌ | ❌ |
| **配额维度** | rpm/tpm/concurrent/budget/lifetime + dry-run | rpm/tpm | rpm/tpm | quota |
| **运行时** | Rust + 编译期 SQL | Python | Go | 闭源 |
| **典型 binary** | 17 MB | ~500 MB image | ~30 MB | n/a |

> 一句话：**用 Rust 把 OneAPI 的产品形态做对，用 manifest 把 LiteLLM 的接入便利搬过来。**

## 30 秒跑通

```bash
docker compose up -d                      # PG + Redis + 迁移 + API + Web 5 服务
open http://localhost:8080                # 控制台（SvelteKit）
# API 在 http://localhost:8000，控制台 client 已编入此地址
```

> 端口分离：UI 在 `:8080`，API 在 `:8000`。生产请用 nginx/caddy 收口到同域。CORS permissive 默认开，跨端口浏览器可调。

要走"自己编译"的，看 [快速开始（手动）](#快速开始手动) 章节。

## 文档地图

入门 / 部署：

- [docs/getting-started.md](./docs/getting-started.md) — 三档接入：30 秒 Docker / 5 分钟 Helm / 10 分钟本地源码
- [RELEASE.md](./RELEASE.md) — 发布、回滚、smoke runbook

架构 / 设计：

- [DESIGN.md](./DESIGN.md) — 领域模型、运行时边界、数据流
- [docs/architecture.md](./docs/architecture.md) — C4 架构总览（control / data / worker plane）
- [docs/architecture/decisions/](./docs/architecture/decisions/) — ADR（架构决议）：ADR-0001/0004/0005/0007 Accepted · ADR-0002/0003 Superseded · ADR-0006 Proposed

扩展面：

- [docs/plugin-manifest.md](./docs/plugin-manifest.md) — HTTP Plugin manifest 规范与示例
- [docs/wasm-plugin-abi.md](./docs/wasm-plugin-abi.md) — WASM Plugin ABI v0 完整设计与实装对账
- [docs/wasm-sdk-as.md](./docs/wasm-sdk-as.md) — AssemblyScript SDK 用法
- [docs/manifest-registry-signature.md](./docs/manifest-registry-signature.md) — Registry 签名 schema

API / 接入：

- [docs/api-reference.md](./docs/api-reference.md) — OpenAPI / Postman / Bruno + 关键 API 索引
- [examples/](./examples/) — SDK / curl / Postman / Bruno / OpenAPI / Terraform / Helm

可观测 / 运维：

- [docs/observability.md](./docs/observability.md) — Prometheus / Grafana / OTLP
- [docs/observability-runbook.md](./docs/observability-runbook.md) — SLO 指标 / 故障处置
- [docs/security-runbook.md](./docs/security-runbook.md) — 密钥轮换 / master key 丢失
- [docs/wasm-runbook.md](./docs/wasm-runbook.md) — WASM 模块故障处置
- [docs/threat-model.md](./docs/threat-model.md) — 威胁模型

路线 / 缺口：

- [ROADMAP.md](./ROADMAP.md) — 四里程碑路线（M1/M2/M3 已交付，M4 候选）
- [docs/product-gaps.md](./docs/product-gaps.md) — v0.4.60 → v0.5.0 产品化缺口对账

## 当前版本：v0.5.0-rc2

> 0.4.x 系列（188 个 patch · 四刀 product-review + 阶段小版收口）已折叠归档。
> 完整路线请按下面顺序：
>
> - 主线 changelog 汇总 → [CHANGELOG.md § 0.4.x](./CHANGELOG.md#04x--2026-05-22-至-2026-05-28--refactor--product-gaps-closure-188-patches)
> - 完整 0.4.NNN 流水 → [docs/archive/changelog/CHANGELOG-0.4.x-patch-log.md](./docs/archive/changelog/CHANGELOG-0.4.x-patch-log.md)
> - 四刀自审历史 → [docs/archive/2026-05-product-reviews/](./docs/archive/2026-05-product-reviews/)
> - 旧 ROADMAP 完整快照 → [docs/archive/roadmap/ROADMAP-pre-0.5.0.md](./docs/archive/roadmap/ROADMAP-pre-0.5.0.md)

v0.5.0 的路线已切换为号池中台叙事（健康度自愈 / 合规过滤模块库 / 难接入渠道标杆），详见 [ROADMAP.md](./ROADMAP.md) § M5/M6/M7。

### 测试基线

| 维度 | 数量 |
|------|-----|
| Rust workspace | 556+ tests |
| Web vitest | 111 cases（19 files，0.5.0 K1 砍 Playground 后）|
| Migrations | 35 SQL files |

## 核心能力

完整能力清单与运行时边界见：

- [docs/architecture.md](./docs/architecture.md) — C4 架构总览（control / data / worker plane）
- [DESIGN.md](./DESIGN.md) — 领域模型、数据流、关键决议
- [ROADMAP.md](./ROADMAP.md) — 当前基线与三里程碑路线

一句话按层：

| 层 | 能力 |
|----|------|
| 租户 | Org × Project × ApiKey 三层 + RBAC + Postgres RLS 兜底 |
| 网关 | OpenAI 兼容 chat/embeddings/images/audio/responses，流式 SSE + tool calling |
| 渠道接入 | HTTP Plugin manifest v1 + 55 provider preset |
| 路由 | priority / weighted_random / round_robin / least_conn / least_latency + fallback + canary |
| 计费 | 多维度定价 + LiteLLM 自动同步 + crash-safe pre-debit + ledger + invoice 状态机 |
| 配额 | rpm / tpm / concurrent / daily / monthly / lifetime + dry-run / explain |
| 身份 | Argon2id + JWT + API Key SHA-256 + OIDC SSO + refresh session 轮转 |
| 可视化 | SvelteKit 控制台 |
| 运维 | `kgctl` CLI + Docker Compose + Prometheus + OpenTelemetry + incident UI |

## 技术栈

| 层 | 选型 |
|---|---|
| Backend | Rust 2024 · Axum 0.7 · Tokio · sqlx 0.8 |
| Storage | PostgreSQL 15+（可选 TimescaleDB）· Redis (fred) |
| Frontend | SvelteKit 2 · Svelte 5 · TypeScript · Tailwind v4 |
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
├── CONTRIBUTING.md
├── SECURITY.md
├── docs/                       # 文档索引 / runbooks / waivers / stages / product-gaps
├── crates/
│   ├── gate-core/              # 领域类型（强类型 ID / Identity / RBAC / Quota）
│   ├── gate-crypto/            # Envelope encryption + KMS 抽象
│   ├── gate-storage/           # PostgreSQL Repository (Pg + InMemory)
│   │   └── migrations/         # 35 SQL 文件，含 RLS / pricing_rules / inflight recovery / request log retention
│   ├── gate-auth/              # Password / JWT / API Key / OIDC / AuthContext
│   ├── gate-cache/             # Redis Lua（rate limit + quota）
│   ├── gate-providers/         # Provider trait + 9 adapters + ProviderRouter + WASM 集成
│   ├── gate-billing/           # Outbox + Multi-dimensional Pricing + LiteLLM sync
│   ├── gate-wasm/              # WASM runtime（wasmtime 26 + 3 hook + fallback + Prometheus）
│   ├── gate-wasm-sdk/          # Rust SDK：写 wasm transform 用
│   ├── gate-server/            # Axum HTTP 网关（主二进制）
│   └── kgctl/                  # 部署运维 CLI
├── sdks/
│   └── gate-wasm-sdk-as/       # AssemblyScript SDK 包（@kooix-gate/wasm-sdk-as）
├── deploy/
│   ├── helm/gate/              # Helm chart（values + templates）
│   └── grafana/dashboards/     # Grafana dashboard JSON
├── bench/                      # 50k rpm 负载测试 + mock upstream
├── examples/                   # SDK / curl / Postman / Bruno / OpenAPI / Terraform / Helm / manifest-packages
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
> 计划轮换 JWT 时，新 key 放 `KOOIX_JWT_SECRET`，旧 key 临时放 `KOOIX_JWT_PREVIOUS_SECRETS`（逗号分隔，仅验签窗口）。

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
# 可选：ProviderRouter 解密后的 channel key 短缓存 TTL；0 表示禁用，默认 30s。
export KOOIX_CHANNEL_KEY_CACHE_TTL_SECS=30

kgctl migrate
kgctl doctor    # 校验 env / JWT rotation window / migration / Redis Lua 全绿
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

### SSO Provider 管理

平台管理员控制台提供 `/admin/sso`，对应 API：

- `GET/POST /v1/admin/identity-providers`
- `PUT/DELETE /v1/admin/identity-providers/:id`
- `POST /v1/admin/identity-providers/discover`
- `GET /v1/auth/sso/providers`（公开，仅返回 enabled 平台级 Provider 的 `name/slug`）

OIDC `client_secret` 使用 `KOOIX_MASTER_KEY` 派生的 envelope encryption 加密，AAD 绑定 `identity_providers.id`，API 与 audit 都不回显明文或密文。Provider 配置支持 OIDC discovery、邮箱域 allowlist、JIT auto-create、auto-join role 与 redirect policy；SSO `redirect_to` 默认只允许相对路径，绝对 URL 必须命中 Provider 的 `allowed_origins`。

### 邀请流

Org / Project 页面提供成员邀请面板，对应 API：

- `GET/POST /v1/admin/orgs/:org_id/invitations`
- `DELETE /v1/admin/orgs/:org_id/invitations/:invitation_id`
- `GET/POST /v1/admin/orgs/:org_id/projects/:project_id/invitations`
- `DELETE /v1/admin/orgs/:org_id/projects/:project_id/invitations/:invitation_id`
- `POST /v1/invitations/preview`、`POST /v1/invitations/accept`（公开接受邀请）

邀请明文 token 只在创建响应中返回一次，数据库只保存 `token_hash=SHA-256(token)`；过期、已接受或已撤销的邀请不能再次接受。Org 邀请要求 `Permission::OrgMemberInvite` / revoke 要求 `OrgMemberRemove`，Project 邀请要求 `ProjectMemberInvite` / revoke 要求 `ProjectMemberRemove`；接受 Project 邀请时会重新读取 Project 所属 Org，写入带 `(OrgId, ProjectId)` 复合上下文的 `project_memberships`，避免跨 Org project ID 重放。

### SCIM 评估边界

P1.7 已完成 SCIM 2.0 评估，结论见 [docs/backlog/scim-evaluation.md](./docs/backlog/scim-evaluation.md)：SCIM 应作为 vNext 的 Org-scoped inbound provisioning connector，负责企业用户同步与 group → role mapping，不授予平台级角色。用户以 email + 独立 `externalId` binding 幂等同步；deprovision 默认 suspend user 并撤销 refresh sessions；group 不能直接等于 role，必须由管理员显式配置到 Org / Project role，Project mapping 必须校验所属 Org。

## 设计要点速览

- **多 Org 三层租户**：Org → Project → ApiKey，永不耦合
- **project_memberships 用 `(OrgId, ProjectId)` 复合 key**：防止跨 Org 重放
- **RLS 兜底**：应用层漏 filter 也泄露不了租户数据
- **流式计费正确性**：`stream_options.include_usage` 强制注入，末帧捕获 usage 后写 outbox；worker 批量落 `usage_records` projection + `billing_ledger_events.actual_settle`
- **Outbox pattern**：业务事务和计费写入解耦，支持 `enqueue_batch`、批量 settlement、批量 mark done，并用幂等 `ON CONFLICT DO NOTHING` 兜底重复事件
- **Billing ledger / invoice**：`billing_ledger_events` 是审计源；月账单状态机 `draft -> closed -> exported -> paid/waived`，导出 digest 绑定审计留存
- **Channel 平台级 + Group 编排**：运营和租户解耦，channel_keys envelope encrypted
- **HTTP Plugin 整流**：`provider_type=plugin` 时 `model_mapping.plugin` 作为 manifest，声明 chat / embeddings request path/body/header、非流式 response path、embedding vector/usage path、流式 SSE event/token/usage path，统一归一为 OpenAI-compatible `ChatResponse` / `ChatStreamChunk` / `EmbeddingResponse`；manifest 按不可信配置处理，默认不允许绝对 URL，模板变量、outbound allowlist、DNS rebind guard、header redaction 与 body/response/SSE size 均有限制
- **Provider 插件预设**：`model_mapping.plugin.preset.provider` 可选 `openai`、`openai_compatible`、`anthropic_messages`、`azure_openai`、`vertex_openai`、`gemini`、`deepseek`、`mistral`、`cohere_chat`、`ollama`、`groq`、`together`、`openrouter`、`moonshot`、`zhipu`、`qwen`、`yi`、`bedrock_converse` 等，把主流 Provider 也收敛到同一 plugin manifest 接入面
- **强类型 ID**：编译期阻止 `OrgId` 当 `UserId` 传
- **typed ID 边界**：API response 序列化为 `{prefix}_{uuid_simple}`；route extractor 通过 `FlexUuid` 同时接收 typed ID 和裸 UUID，数据库仍存裸 UUID
- **AuthContext 单一权限门面**：禁止外部读 raw 角色映射，全走 `can()` / `require!`
- **多维度计费**：按 dimension × conditions 精准匹配，支持缓存折扣、批量折扣、分层定价
- **crash-safe pre-debit**：budget quota 先 Redis 预扣，再把 `quota_keys` / `estimated_micros` 写入 `inflight_requests`；正常 settle 多退少补，异常 drop 全退，进程崩溃由 sweeper 兜底

详细架构见 [DESIGN.md](./DESIGN.md)，HTTP Plugin manifest 示例见 [docs/plugin-manifest.md](./docs/plugin-manifest.md)。文档总入口见 [docs/README.md](./docs/README.md)，已完成的阶段性审计与收口记录统一放在 [docs/stages/](./docs/stages/)。SDK / curl / Postman / Bruno / OpenAPI / Terraform / Helm 示例见 [examples/](./examples/)。

## 测试

```bash
# 全量（当前 556+ Rust + 127 web tests，含 testcontainers 集成测试，需要 Docker）
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
