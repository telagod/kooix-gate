# gate-core

Kooix Gate 领域核心：强类型 ID、Identity、RBAC、Quota schema。无 I/O，无 storage 依赖，纯类型与值对象。

## 关键模块

- `id` — `OrgId` / `UserId` / `ProjectId` / `ChannelId` / `ApiKeyId` 等强类型 ID（编译期防串台 + `Display` 输出 `{prefix}_{uuid_simple}` + `FlexUuid` 兼容裸 UUID）
- `identity` — `Identity` / `AuthContext` 单一权限门面（外部禁直接读 raw 角色映射，全走 `can()` / `require!`）
- `rbac` — Org / Project 角色枚举 + Permission set
- `quota` — 维度 / scope / dry-run 类型
- `time` — chrono 包装

## 边界

- 不持任何状态
- 不依赖 sqlx / fred / reqwest
- 所有 trait `Sync + Send + 'static`，便于 Axum extractor 复用

详见 [DESIGN.md § 1 领域模型](../../DESIGN.md#1-领域模型) 与 [§ 2 RBAC 设计](../../DESIGN.md#2-rbac-设计)。
