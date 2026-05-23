# gate-storage

PostgreSQL 持久化层：Repository trait（`Pg` + `InMemory` 双实现）+ 编译期 SQL 校验（sqlx 0.8）+ migrations。

## 子模块

- `pg/` — `PgRepo`，编译期 SQL，Postgres 17（testcontainers tag 可用 `KOOIX_TEST_PG_TAG` 覆盖）
- `mem/` — `InMemoryRepo`，单元测试与 dev mode 用
- `migrations/` — 35 个 SQL，含 RLS / `pricing_rules` / `inflight_requests` / `request_log_events` 月分区投影 / retention helpers
- `tests/pg_repo.rs` — testcontainers Postgres 集成测试
- `tests/rls_isolation.rs` — RLS 兜底验证

## 注意

- 新增 migration 后跨 crate 测试**必须** `cargo clean -p gate-storage`，否则 sqlx prepare cache 会卡旧 schema
- 数据库存裸 UUID；typed ID 序列化在 `gate-core::id`
- 所有 repo 方法接受 `&AuthContext` 用于审计 / RLS scope

详见 [DESIGN.md § 4 数据流](../../DESIGN.md) 与 [docs/architecture/data-plane.md](../../docs/architecture/data-plane.md)。
