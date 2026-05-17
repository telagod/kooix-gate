# Kooix Gate

> Rust + Svelte 打造的 LLM 网关。多 Org 三层租户、9 Provider 多模态、流式正确计费、可视化编排、配额拦截、SSO/OIDC。

竞品定位：NewAPI / OneAPI / LiteLLM 的「底盘加强版」——把它们反复踩的雷（权限粗、限流单一、租户隔离漏、流式漏扣）先治好，再谈渠道接入。

[![Tests](https://img.shields.io/badge/tests-241%20passed-brightgreen)](#测试)
[![Rust](https://img.shields.io/badge/rust-2024-orange)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue)](./LICENSE)

## 当前版本：v0.1.5

详见 [CHANGELOG.md](./CHANGELOG.md)。

### 核心能力

- ✅ 多 Org × Project × ApiKey 三层租户 + RBAC + RLS 兜底
- ✅ 9 Provider 适配（OpenAI / Anthropic / Azure / Gemini / DeepSeek / Mistral / Groq / Moonshot / Bedrock）
- ✅ `/v1/chat/completions` OpenAI 兼容（流式 SSE + 非流式 + tool calling）
- ✅ 5 种路由策略（priority / weighted_random / round_robin / least_conn / least_latency）
- ✅ 多维度计费引擎（token / image / audio / cache / batch，自动同步 LiteLLM 定价）
- ✅ Quota 拦截（rpm / tpm / budget，Redis Lua 原子）
- ✅ 可视化编排 Playground（@xyflow/svelte 节点式流程编辑器）
- ✅ SvelteKit 控制台（channel 管理 / 请求日志 / usage 仪表盘 / 月度账单 / SSO）
- ✅ `kgctl` 部署 CLI + Docker Compose 一键部署 + GitHub Actions CI

### v0.1.5 新增亮点

- 🔌 9 Provider 插件架构 + tool calling + embeddings
- 🎯 5 种路由策略 + model filter + channel RPM/TPM 限速 + 自动禁用
- 💰 多维度计费引擎 + LiteLLM 自动同步定价
- 🎨 节点式可视化编排 Playground
- 📊 请求日志 20+ 维度过滤 + Dashboard 统计面板
- 🎭 全面 UI 重做：monochrome 设计系统 + dark mode + 品牌色 logo

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
├── crates/
│   ├── gate-core/              # 领域类型（强类型 ID / Identity / RBAC / Quota）
│   ├── gate-crypto/            # Envelope encryption + KMS 抽象
│   ├── gate-storage/           # PostgreSQL Repository (Pg + InMemory)
│   │   └── migrations/         # 24 SQL 文件，含 RLS
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
kgctl seed-pricing
kgctl admin create --email you@example.com
kgctl doctor    # 体检全绿再起服务
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

安全边界：所有 `/v1/admin/users*` mutation 必须通过 `Permission::PlatformAdmin`；`login` 与 `refresh` 都会检查用户当前状态，非 `active` 用户无法获得新 token。关键 mutation 写入 audit action：`user.create`、`user.update_status`、`user.reset_password`。

## 设计要点速览

- **多 Org 三层租户**：Org → Project → ApiKey，永不耦合
- **project_memberships 用 `(OrgId, ProjectId)` 复合 key**：防止跨 Org 重放
- **RLS 兜底**：应用层漏 filter 也泄露不了租户数据
- **流式计费正确性**：`stream_options.include_usage` 强制注入，末帧捕获 usage 后 spawn 推 outbox
- **Outbox pattern**：业务事务和计费写入解耦，幂等 `ON CONFLICT DO NOTHING`
- **Channel 平台级 + Group 编排**：运营和租户解耦，channel_keys envelope encrypted
- **强类型 ID**：编译期阻止 `OrgId` 当 `UserId` 传
- **AuthContext 单一权限门面**：禁止外部读 raw 角色映射，全走 `can()` / `require!`
- **多维度计费**：按 dimension × conditions 精准匹配，支持缓存折扣、批量折扣、分层定价

详细架构见 [DESIGN.md](./DESIGN.md)。

## 测试

```bash
# 全量（241 tests，含 testcontainers 集成测试，需要 Docker）
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
cd web && npm run build
```

## License

[AGPL-3.0-only](./LICENSE)
