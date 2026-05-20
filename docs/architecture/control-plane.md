# Control Plane

Status: active
Scope: auth、Org/Project/API Key、Channel、Quota、Billing、Usage、Admin、SSO、invitation 等管理面。
Last verified: 2026-05-21

## 职责

Control plane 负责：

- 用户与 session
- Org / Project / ApiKey 生命周期
- Channel / Channel group / Provider preset / Plugin manifest
- Quota 解释与对账
- Billing / Usage / Request logs
- SSO / invitation / admin incidents

## 关键约束

- 所有 mutation 走 RBAC + Scope。
- secret 通过 KMS / encrypted slot 管理。
- typed ID 必须保留为 API response 的稳定前缀。
- 高风险操作要可审计、可回滚、可确认。

## 代码锚点

- `crates/gate-server/src/app.rs`
- `crates/gate-server/src/routes/mod.rs`
- `crates/gate-server/src/routes/auth.rs`
- `crates/gate-server/src/routes/sso.rs`
- `crates/gate-server/src/routes/projects.rs`
- `crates/gate-server/src/routes/api_keys.rs`
- `crates/gate-server/src/routes/channels.rs`
- `crates/gate-server/src/routes/quotas.rs`
- `crates/gate-server/src/routes/billing.rs`
- `crates/gate-server/src/routes/usage.rs`
- `crates/gate-server/src/routes/request_logs.rs`
- `crates/gate-server/src/routes/admin`
