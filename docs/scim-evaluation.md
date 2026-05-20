# SCIM 2.0 评估

Status: evaluated for vNext implementation
Scope: P1.7 Identity / Enterprise；只评估用户同步与 group → role mapping，不声明当前已提供 SCIM runtime endpoints
Last verified: 2026-05-20

## 结论

Kooix Gate 当前已经具备 SCIM 落地所需的身份底座：全局 `users`、Org / Project membership、SSO JIT、邀请流、refresh session 撤销与 `JwtRing`。但不应在 P1.7 里直接补“半套 SCIM API”。推荐先把 SCIM 定义为 **vNext 的 Org-scoped inbound provisioning connector**：

- SCIM 只同步企业租户内用户与组，不授予平台级 `PlatformRole`。
- 用户同步以 `users.email` 为主匹配键，`externalId` 必须落独立绑定表，不能复用 OIDC `sub`。
- Group 不直接等于角色；必须先由管理员配置显式 mapping，再把 SCIM group membership 投影到 `org_memberships` / `project_memberships`。
- Deprovision 首选 `users.status = suspended` + revoke refresh sessions；删除 / 硬删不进入默认链路。
- Membership 删除必须有来源追踪，只能撤销 SCIM 自己授予的角色，避免误删本地手工 Owner / Admin。

因此，P1.7 的验收项“SCIM 评估”已收敛为以下实现蓝图；真正 endpoints、migration 与 UI 应作为 P1.8/P2 安全实现项继续推进。

## 当前身份模型可复用点

| 领域 | 当前 artifact | SCIM 复用方式 |
| --- | --- | --- |
| 全局用户 | `users` / `UserRepo` | `emails[primary].value` 或 `userName` 归一为小写邮箱；`displayName` 同步到 `display_name`。 |
| 用户状态 | `UserStatus::{Active,Suspended,PendingVerification,Deleted}` | SCIM `active=false` 映射为 `suspended`；不默认映射为 `deleted`。 |
| Org membership | `org_memberships` / `MembershipRepo::add_org_member` | Group mapping 可投影到 `OrgRole::{owner,admin,billing_viewer,member}`，但 `owner` 必须显式允许。 |
| Project membership | `project_memberships` + `(OrgId, ProjectId)` 权限上下文 | Group mapping 投影到 `ProjectRole::{owner,admin,developer,viewer}` 时必须包含 Org 上下文，延续跨 Org 重放防线。 |
| SSO | `identity_providers` / `user_identities` | SCIM 不应塞进 OIDC provider 表；应新建 SCIM source / binding 表，避免 subject 语义混淆。 |
| Invitation | `invitations` | 邀请流继续处理人工接入；SCIM 处理 IdP 主导的自动 provisioning，两者并行但不能互相覆盖。 |
| Session | `user_sessions` | SCIM deprovision 后撤销 refresh sessions，阻断后续 refresh；access token 等自然过期或走 emergency JWT rotation。 |

## 用户同步评估

### 入站对象

建议首版只接受 SCIM core User schema 的最小字段：

- `id`：Kooix 返回的 SCIM resource id，可使用内部 `usr_...` 或独立 `scim_user_id`；不要暴露裸 UUID。
- `externalId`：企业 IdP 侧稳定 ID，必填或在创建后强制保存；用于幂等重放。
- `userName`：推荐要求为邮箱；若 IdP 不能保证，则必须提供 primary email。
- `emails[]`：至少一个 primary 或第一个 work email。
- `displayName` / `name.formatted`：可选展示名。
- `active`：`true` → active；`false` → suspended + revoke sessions。

### 归一化规则

1. Email 按现有登录语义小写归一，与 `users.email` 的 `CITEXT UNIQUE` 对齐。
2. 若 `externalId` 已绑定用户，则 PATCH 同步到该用户。
3. 若 `externalId` 未绑定但 email 已存在，则只在配置 `link_existing_by_email=true` 时绑定；否则返回冲突，避免错误接管本地账户。
4. 新建 SCIM 用户默认 `password_hash = NULL`，必须通过 SSO 登录；不通过 SCIM 设置密码。
5. `active=false` 不删除用户，只切 `suspended` 并撤销 refresh sessions；重新 `active=true` 才恢复登录资格。

### 不做事项

- 不支持 SCIM 写入 `PlatformRole`。
- 不支持 SCIM 设置密码、MFA secret 或 API key。
- 不支持 SCIM 硬删 `users`；`DELETE /Users/:id` 应等价为 suspend。
- 不允许 SCIM 自动创建 Org / Project；只能在既有 Org / Project 内同步成员。

## Group → role mapping 评估

### 为什么不能 group 直连 role

SCIM Group 的 `displayName` / `externalId` 来自企业 IdP，语义不可由网关信任。若直接把 group 名称当角色，会出现：

- IdP group 重命名导致权限漂移。
- 同名 group 在不同 Org 间串台。
- 外部管理员误把用户加入高权 group，直接获得 Kooix Owner / Admin。
- 移除 group 时误删本地手工授予的权限。

因此必须使用显式 mapping 表，且 mapping 生效范围限定在单个 Org。

### 推荐 mapping 模型

```text
scim_connections
  id, org_id, name, token_hash, enabled, allowed_ip_cidrs, created_at, updated_at

scim_user_links
  connection_id, user_id, external_id, last_seen_at, raw_user

scim_group_mappings
  id, connection_id, external_group_id, display_name,
  target_kind = 'org' | 'project',
  target_org_id,
  target_project_id nullable,
  role,
  allow_owner boolean default false,
  enabled boolean

scim_membership_grants
  mapping_id, user_id, granted_role, last_seen_at
```

投影规则：

- `target_kind=org` → `org_memberships(org_id, user_id, role)`。
- `target_kind=project` → 先校验 `projects.org_id == target_org_id`，再写 `project_memberships(project_id, user_id, role)`。
- `owner` 角色必须 `allow_owner=true` 且二次确认；默认拒绝。
- 多个 group 命中同一 target 时取最高有效角色，但降权 / 删除只能处理 `scim_membership_grants` 证明由 SCIM 授予的那部分。
- 移除 group membership 后，若没有其它 SCIM mapping 支撑该 target，则撤销 SCIM grant；但不得删除最后一个 Org Owner。

## API 与运行时边界建议

首版 SCIM endpoints 建议独立于 admin API，使用企业 connector token：

```text
GET    /scim/v2/:connection_id/ServiceProviderConfig
GET    /scim/v2/:connection_id/Schemas
GET    /scim/v2/:connection_id/ResourceTypes
GET    /scim/v2/:connection_id/Users
POST   /scim/v2/:connection_id/Users
GET    /scim/v2/:connection_id/Users/:id
PATCH  /scim/v2/:connection_id/Users/:id
DELETE /scim/v2/:connection_id/Users/:id
GET    /scim/v2/:connection_id/Groups
PATCH  /scim/v2/:connection_id/Groups/:id
```

安全边界：

- Bearer token 只存 SHA-256 hash，创建响应显示一次，和 invitation token 策略一致。
- 每个 connection 固定 `org_id`，所有 read/write 必须带 Org 上下文。
- 支持 IP allowlist / rate limit / request body size limit。
- 所有 mutation 写 audit：`scim.user.create`、`scim.user.patch`、`scim.user.suspend`、`scim.group.apply`、`scim.mapping.apply`。
- Audit detail 记录字段 diff 与 mapping id，不记录 bearer token、raw secret 或超大 raw claims。
- `ListResponse` 必须支持分页，避免 IdP 全量同步拖垮控制面。

## 与当前代码的差距

| 差距 | 影响 | vNext 动作 |
| --- | --- | --- |
| `MembershipRepo` 只有 `remove_org_member`，缺少 project remove 与来源追踪 | SCIM group 移除无法安全撤销 Project role | 增加 source-aware membership grants 或独立 `scim_membership_grants`。 |
| `UserRepo` 没有 SCIM externalId 绑定 | 无法幂等处理 IdP 重放和 email 变更 | 新增 `scim_user_links`，不要复用 OIDC `user_identities`。 |
| `identity_providers.provider_type` 当前只允许 `oidc` / `saml` | 不能把 SCIM connection 塞进现表 | 新建 `scim_connections`，或未来抽象为 `identity_sources`。 |
| route manifest 尚无 SCIM endpoints | 控制台/API client 不知道 SCIM 面 | endpoints 实现时同步 `route_manifest.rs` 与 generated web manifest。 |
| UI 无 group mapping 页面 | 管理员无法安全配置 group → role | 在 `/admin/sso` 或新 `/admin/scim` 增加 mapping UI。 |

## 验收清单

P1.7 “SCIM 评估”完成标准：

- [x] 明确用户同步字段映射、幂等键和 deprovision 策略。
- [x] 明确 group → role 不能直连，必须通过 Org-scoped explicit mapping。
- [x] 明确 Org / Project role 投影规则与跨 Org 防线。
- [x] 明确 token、audit、rate limit、body size 与 secret redaction 安全边界。
- [x] 明确当前代码差距与 vNext migration / API / UI 动作。

下一阶段若进入实现，不得只补 endpoints；必须同时补 migration、repo、route manifest、tests、docs 与 gitleaks 扫描。
