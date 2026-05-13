# Kooix Gate

> Rust + Svelte 打造的 LLM 网关。单机 5w rpm，多 Org 多 Project，插件式渠道。

竞品定位：NewAPI / OneAPI / LiteLLM 的「底盘加强版」——把它们反复踩的雷（权限粗、限流单一、租户隔离漏、流式漏扣）先治好，再谈渠道接入。

## 技术栈

| 层 | 选型 |
|---|---|
| Backend | Rust 2024 · Axum 0.7 · Tokio |
| Storage | PostgreSQL 15+ (TimescaleDB) · Redis (fred) |
| Frontend | SvelteKit 2 · Svelte 5 Runes · shadcn-svelte |
| Observability | tracing + OpenTelemetry |
| Auth | Argon2 + JWT + API Key (SHA-256) |

## 当前状态

第一阶段：基座（schema + 领域类型 + 设计文档）

- ✅ Cargo workspace
- ✅ gate-core 领域类型（强类型 ID / Identity / RBAC / Quota）
- ✅ 完整 SQL 迁移（10 个文件，含 RLS）
- 🚧 gate-storage Repository 实现
- 🚧 gate-auth / gate-server / gate-cache

详细设计见 [DESIGN.md](./DESIGN.md)。

## 项目结构

```
kooix-gate/
├── Cargo.toml                          # workspace
├── DESIGN.md                           # 设计文档（必读）
└── crates/
    ├── gate-core/                      # 领域模型，纯类型
    │   └── src/
    │       ├── id.rs                   # 强类型 ID
    │       ├── identity.rs             # Org / User / Project / Membership
    │       ├── rbac.rs                 # 权限点 + 角色映射
    │       ├── quota.rs                # 配额维度 + 检查
    │       └── error.rs
    └── gate-storage/                   # PostgreSQL 持久层
        ├── src/lib.rs
        └── migrations/
            ├── 20260513000001_extensions.sql
            ├── 20260513000002_identity.sql
            ├── 20260513000003_api_keys.sql
            ├── 20260513000004_channels.sql
            ├── 20260513000005_routing.sql
            ├── 20260513000006_quota.sql
            ├── 20260513000007_usage.sql
            ├── 20260513000008_audit.sql
            ├── 20260513000009_sessions_outbox.sql
            └── 20260513000010_rls.sql
```

## 快速开始（基座阶段）

```bash
# 1. 起 PG（含 TimescaleDB）
docker run -d --name kg-pg \
  -e POSTGRES_PASSWORD=devpass \
  -e POSTGRES_DB=kooix_gate \
  -p 5432:5432 \
  timescale/timescaledb:latest-pg16

# 2. 起 Redis
docker run -d --name kg-redis -p 6379:6379 redis:7-alpine

# 3. 跑迁移
export DATABASE_URL=postgres://postgres:devpass@localhost/kooix_gate
cargo install sqlx-cli --no-default-features --features postgres,rustls
sqlx migrate run --source crates/gate-storage/migrations

# 4. 编译检查
cargo check --workspace
```

## 设计要点速览

- **多 Org 三层租户**：Org → Project → ApiKey，永不耦合
- **RLS 兜底**：应用层漏 filter 也泄露不了租户数据
- **流式扣费三段式**：预扣 → 流中 → 修正，避免热点和漏扣
- **Channel 平台级 + Group 编排**：运营和租户解耦
- **强类型 ID**：编译期阻止串台
- **WASM 插件延后**：trait 抽象先稳定再开 ABI

## License

AGPL-3.0-only
