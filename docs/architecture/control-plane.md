# Control Plane

Status: active
Scope: auth、Org/Project/API Key、Channel、Quota、Billing、Usage、Admin、SSO、invitation 等管理面。
Last verified: 2026-05-22

> 本页面是 control plane 的运行时边界与代码锚点。领域模型的全文论述见 [DESIGN.md §1-2](../../DESIGN.md)。

## 职责

Control plane 负责所有 **管理面** API：把 Org / Project / ApiKey / Channel / Quota / Pricing / Billing / SSO / Invitation 等领域对象的 CRUD + 状态流转封装为 REST API，并写入 audit。

| 子域 | 文件 |
|------|------|
| 用户 / session / refresh | `routes/auth.rs` `routes/admin/users.rs` |
| Org / Project | `routes/projects.rs` `routes/admin/orgs.rs` |
| API Key | `routes/api_keys.rs` |
| Channel + Channel group | `routes/channels.rs` `routes/admin/channels.rs` `routes/admin/groups.rs` |
| Provider preset / Plugin manifest | `routes/admin/plugin_manifest.rs` |
| Pricing rules | `routes/admin/pricing.rs` |
| Quota policies | `routes/quotas.rs` |
| Billing / invoice | `routes/billing.rs` |
| Usage / request logs | `routes/usage.rs` `routes/request_logs.rs` |
| SSO / OIDC | `routes/sso.rs` `routes/admin/sso.rs` |
| Invitation | `routes/invitations.rs` |
| Admin incidents / audit | `routes/admin/incidents.rs` `routes/admin/audit.rs` |

## 关键约束

### 准入

- 所有 mutation 走 `Permission::*` + `Scope::*`：`require!` 或 `can!`，禁止外部读 raw 角色映射（[DESIGN §2.8](../../DESIGN.md#28-权限检查门面)）。
- `AuthContext` 是单一权限门面：handler 只接收 `AuthContext`，不接收 `RoleId` / `OrgRole` / `ProjectRole`。

### 数据隔离

- **三层租户**：Org → Project → ApiKey 永不耦合；`project_memberships` 用 `(OrgId, ProjectId)` 复合 key（[DESIGN §1.1](../../DESIGN.md)）。
- **RLS 兜底**：应用层漏 filter 也泄露不了租户数据。每个 request 在事务起点 `SET LOCAL app.org_id = ...`。
- **typed ID 边界**：API response 序列化为 `{prefix}_{uuid_simple}`；route extractor 用 `FlexUuid` 同时接受 typed ID 与裸 UUID；DB 仍存裸 UUID。

### 安全

- 所有 secret 走 envelope encryption（`KOOIX_MASTER_KEY` 派生），AAD 绑定 owner ID（防密文移植，[DESIGN §7.2](../../DESIGN.md#72-aad-绑定约定防密文移植)）。
- OIDC `client_secret` / channel keys / API keys 都通过 `EnvelopeKms` 加密，audit 不回显明文或密文。
- 高风险 mutation（delete channel / rotate key / suspend user / change pricing / disable group）默认要求二次确认。

### 状态机

- **Channel**：`active` ↔ `inactive` ↔ `draining` ↔ `cooling_down`。`drain` 禁止新请求，`disable_when_idle` 等 inflight 清空后真禁。
- **Invoice**：`draft → closed → exported → paid|waived`，写 audit。
- **User**：`active ↔ suspended ↔ pending_verification`；停用当前登录管理员被拒绝（避免自锁）。

## 代码锚点

- `crates/gate-server/src/app.rs` — Router 装配
- `crates/gate-server/src/state.rs` — AppState（含 ProviderRouter / KMS / repos）
- `crates/gate-server/src/route_manifest.rs` — 路由清单（dataplane vs controlplane vs admin）
- `crates/gate-server/src/routes/mod.rs` — 路由分组入口
- `crates/gate-server/src/routes/auth.rs` `routes/sso.rs`
- `crates/gate-server/src/routes/projects.rs` `routes/api_keys.rs`
- `crates/gate-server/src/routes/channels.rs`
- `crates/gate-server/src/routes/quotas.rs`
- `crates/gate-server/src/routes/billing.rs`
- `crates/gate-server/src/routes/usage.rs`
- `crates/gate-server/src/routes/request_logs.rs`
- `crates/gate-server/src/routes/invitations.rs`
- `crates/gate-server/src/routes/admin/` — 平台管理员路由
- `crates/gate-storage/src/` — Repo trait + Pg impl（编译期 SQL）
- `crates/gate-auth/src/` — Password / JWT / API Key / OIDC

## 跨页面交叉

- 数据流入 → [Data Plane](./data-plane.md)
- 异步任务 → [Worker Plane](./worker-plane.md)
- ADR 决议 → [docs/architecture/decisions/](./decisions/)
