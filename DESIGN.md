# Kooix Gate · 设计文档

> 一个不会越长越烂的 LLM 网关基座。

## 0. 设计原则

1. **底盘优先**：身份、权限、配额、路由——这四块决定上限，先于渠道接入。
2. **多 Org 第一公民**：`Org → Project → ApiKey` 三层，永不混淆。
3. **强类型 ID**：编译期防止 `OrgId` 当 `ProjectId` 传。
4. **租户隔离两道闸**：应用层 `WHERE org_id` + 数据库 RLS 兜底。
5. **插件即 trait**：编译期 trait，运行期 WASM，对上层透明。
6. **预扣 + 修正**：流式扣费三段式，避免热点 + 漏扣。
7. **审计同步落，用量异步落**：区分关键事件与高频事件。

---

## 1. 领域模型

```
                       ┌──────────────┐
                       │ Organization │ (顶层租户，计费主体)
                       └──────┬───────┘
                              │ 1:N
              ┌───────────────┼──────────────┐
              ▼               ▼              ▼
        ┌─────────┐    ┌──────────┐   ┌─────────────┐
        │ OrgRole │    │ Project  │   │ Invitation  │
        │ Member  │    └────┬─────┘   └─────────────┘
        └─────────┘         │ 1:N
                            ▼
                    ┌───────────────┐
                    │ ProjectMember │ (M:N → User)
                    │ ApiKey        │
                    │ ModelAlias    │
                    │ Quota[]       │
                    │ AuditLog[]    │
                    └───────────────┘
                            │ M:N
                            ▼
                    ┌───────────────┐         ┌──────────┐
                    │ ChannelGroup  │ ◄─M:N─► │ Channel  │ (平台级)
                    │ (路由策略)    │         │ + KeyPool│
                    └───────────────┘         └──────────┘
```

### 1.1 三层租户的边界

| 层 | 职责 | 关键字段 |
|---|---|---|
| **Org** | 合同/计费/合规主体 | `slug` 全局唯一，`owner_user_id` |
| **Project** | 隔离边界、成本归属、配额单元 | `(org_id, slug)` 唯一 |
| **ApiKey** | 调用凭证 | `project_id` 强绑定，`allowed_models[]` 缩范围 |

**为什么这么分？**
- Org 拆出来是为了未来转 SaaS：一个公司可能有多个事业部，每个事业部独立计费但共享品牌。
- Project 是隔离主体：dev / staging / prod 各一个 Project，配额和成本天然分开。
- ApiKey 强绑 Project：避免 newapi 那种 "key 越权调用其他用户模型" 的雷。

### 1.2 渠道与项目的解耦

**Channel 是平台资源**，由 `platform_admin` 创建——运营运维各管各的。
**ChannelGroup 是路由编排单元**，把多个 Channel 按策略组合（priority / weighted / fallback / round_robin / least_latency）。
**Project 通过 ProjectGroupBinding 选择能用哪些 Group**——可按 model_pattern 进一步细分。

这层抽象的好处：
- 新增渠道 → 不动 Project 配置
- 切换主备 → 改 Group 内的 priority
- 黑产防控 → 单个 Project 解绑 Group 即停服，不影响其他

---

## 2. RBAC 设计

### 2.1 为什么不用 Casbin/Cedar

- 你的角色组合是**有限的**（Org 4 个 + Project 4 个 + Platform 3 个）
- ABAC 复杂规则用不上，自建 RBAC 编译期映射够快够清晰
- 真到需要时再换，trait 隔离了下层实现

### 2.2 角色矩阵（节选）

| 权限 / 角色 | OrgOwner | OrgAdmin | ProjOwner | ProjAdmin | ProjDev | ProjViewer |
|---|---|---|---|---|---|---|
| `org.update` | ✓ | ✓ | | | | |
| `org.billing.write` | ✓ | | | | | |
| `org.member.invite` | ✓ | ✓ | | | | |
| `project.create` | ✓ | ✓ | | | | |
| `project.delete` | ✓ | ✓ | ✓ | | | |
| `project.member.invite` | ✓ | ✓ | ✓ | ✓ | | |
| `apikey.create` | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `apikey.revoke` | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `quota.write` | ✓ | ✓ | ✓ | ✓ | | |
| `usage.read` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `audit.read` | ✓ | ✓ | ✓ | ✓ | | |

完整映射在 `crates/gate-core/src/rbac.rs`。

### 2.3 权限检查门面

所有路由强制走：

```rust
require!(ctx, Permission::ApiKeyCreate, Scope::Project { org, project });
```

不允许在 handler 里手写 if 判断——杜绝散落漏检。

---

## 3. 限流 + 配额模型

### 3.1 分两类，不要混

| 类型 | 性质 | 算法 | 触发后 | 存储 |
|---|---|---|---|---|
| **Rate Limit** | 频率保护 | Redis 滑动窗口 (ZSET + Lua) | 429 | Redis 主存 |
| **Quota / Budget** | 用量配额 | 原子计数 + 周期重置 | 402 | Redis 计数 + PG 持久化 |

### 3.2 多维叠加（取最严）

每个请求依次过：
```
Platform 全局 → Org → Project → User × Model → ApiKey × Model
```

任一维度 deny 即拒。维度内多条规则取**更严**的。

### 3.3 流式扣费三段式

```
PRE  (请求接入):
  estimated = max_tokens × output_price
  redis.eval('check_and_deduct.lua', quota_keys, estimated)
  if denied → 429/402
  insert into inflight_requests

STREAM (流式中):
  仅累加展示用 token 计数，不动配额（避免热点）

POST (流结束 / 错误):
  真实 usage 回来
  redis.eval('adjust.lua', quota_keys, real - estimated)
  delete from inflight_requests
  enqueue outbox_events('usage', record)
```

后台 worker 消费 outbox → 落 `usage_records`、触发告警、对账。

**关键：超时回滚**。`inflight_requests.expires_at` 设 `max_tokens / 10 tps` 的预估时间，cleaner 定时跑回滚。

---

## 4. Channel & Key 池

### 4.1 健康熔断

每个 `channel_keys.health` 状态机：

```
healthy ──5次连续错误──→ cooling_down (60s)
                            │
                            ├──冷却后探活成功──→ healthy
                            └──探活失败──→ cooling_down (×2 指数退避)

任意状态 ──手动──→ disabled
```

Channel 级 `health` 由所有 key 状态聚合：
- 全部 healthy → healthy
- 部分 cooling_down → degraded
- 全部 cooling_down/disabled → unhealthy（触发 Group fallback）

### 4.2 路由策略

| Strategy | 选择算法 |
|---|---|
| `priority` | 按 priority 升序，第一个 healthy 的 channel |
| `weighted` | 按 weight 加权随机（健康 key 中） |
| `round_robin` | 轮询（健康 key 中） |
| `fallback` | priority 主，挂了切 fallback_group |
| `least_latency` | 最近 5 分钟 P50 最低 |

---

## 5. 数据库决策

### 5.1 PostgreSQL 单实例 vs 集群

- 3000 用户、5w rpm 的写入主要在 `usage_records`（21 亿行/月）
- **配置类表**（users / projects / channels …）QPS 很低，单实例几十年都跑得动
- **usage_records 必须 TimescaleDB hypertable**，按天分块 + 7 天压缩 + 90 天保留

### 5.2 Row-Level Security 兜底

```sql
SET LOCAL app.current_org_id = '<uuid>';
SET LOCAL app.is_platform_admin = 'false';
```

应用层每个请求开始时设置，所有 `project` 范围表自动 `WHERE org_id = current_org_id()`。

应用层漏写 filter 也不会泄露——多一道保险。

### 5.3 强类型 ID

`gate-core::id` 用宏生成 `OrgId/UserId/ProjectId/...`，编译期防止串台。`Display` 给前缀（`org_xxx`, `proj_xxx`），日志友好。

---

## 6. 关键决策记录（ADR-style）

| # | 决策 | 理由 |
|---|---|---|
| 1 | 多 Org 设计直接落 | 单 Org 后期加 Org 列代价极高，先做不亏 |
| 2 | sqlx 不用 ORM | 编译期校验 SQL，ORM 的抽象在这类场景反受其累 |
| 3 | RLS 兜底 | 应用层 bug 不可避免，DB 多一道隔离 |
| 4 | Channel 平台级 | 运营与租户解耦，符合 SaaS 心智 |
| 5 | 流式三段式扣费 | 实时扣 → 热点；事后扣 → 漏扣；预扣 + 修正是唯一解 |
| 6 | TimescaleDB | 5w rpm × 30 天 = 21 亿行，普通表会卡 |
| 7 | WASM 插件延后 | trait 抽象先稳定，ABI 一旦发出去难改 |
| 8 | 自建 RBAC | 角色组合有限，Casbin 引入心智税不值 |

---

## 7. 安全运维注记（必读）

### 7.1 Master Key 备份策略

`KOOIX_MASTER_KEY` 是 envelope encryption 的 KEK，**丢失等于所有渠道密钥/OIDC client_secret 全部失效**。

部署流程：

```bash
# 1. 生成密钥（一次性）
cargo run -p kgctl -- init > deploy/secrets.env

# 2. 立刻分发三处保存
#    a. 密码管理器（1Password / Bitwarden 团队保险柜）
#    b. 云 KMS / Vault Transit
#    c. 物理离线备份（U 盘 + 保险箱）

# 3. 部署后用 echo $KOOIX_MASTER_KEY 验证已注入
# 4. secrets.env 立即从磁盘清除：shred -u deploy/secrets.env
```

**轮换策略**：master key 轮换需要重新加密所有 `*_enc` 列。当前未实现轮换工具，路线图上 P1。

### 7.2 AAD 绑定约定（防密文移植）

所有加密字段**必须用 `gate_crypto::aad` 模块的助手生成 AAD**，杜绝凭空写约定字符串：

| 加密字段 | AAD 助手 | 防御目标 |
|---|---|---|
| `channels.config_enc` | `aad::channel_config(channel_id)` | DBA 把 A 渠道的配置挂到 B 渠道复用 |
| `channel_keys.key_enc` | `aad::channel_key(channel_key_id)` | 把废弃 key 的密文搬回活跃位置 |
| `identity_providers.client_secret_enc` | `aad::idp_secret(provider_id)` | 把测试 IdP 的 secret 嫁接到生产 IdP |

写入 / 读取必须用同一个 AAD。AAD 不需保密，写错时 AEAD 验证会直接 `AeadFailed`——肉眼可见的报错好过静默泄露。

### 7.3 JWT Secret 纪律

- `JwtIssuer::new` 编译期强制 secret >= 32B（短了直接 `AuthError::Invalid`）
- 生产用 64B：`cargo run -p kgctl -- key jwt`
- 轮换 = 所有现有会话立即失效（用户被踢下线）。窗口期可同时验证新旧两把：在 `JwtIssuer` 之上叠一个 `JwtRing`（未实现，路线图）

### 7.4 require! 纪律

**所有 handler 必须以 `require!(ctx, ...)` 开头**。Code review 的 grep 命令：

```bash
# 查找可疑的「绕过授权」写法
rg 'fn.*\(.*AuthContext' --type rust -A 5 | rg -v 'require!|can!|require_user!|require_api_key!'
```

`AuthContext` 内部 `org_memberships` / `project_memberships` / `platform_role` 字段已收为 `pub(crate)`——外部 crate 拿不到 raw 映射，**只能**通过 `can()` / `require()` / 只读 `*_role()` accessor 访问。

`project_memberships` 用复合 key `(OrgId, ProjectId)`——攻击者拿到合法 project_id 替换 Org 上下文重放无效（被 `cross_org_project_id_replay_denied` 测试覆盖）。

### 7.5 部署 env 清单

完整清单见 `cargo run -p kgctl -- env`。最小集：

```
KOOIX_MASTER_KEY    # base64 32B (kgctl key master)
KOOIX_JWT_SECRET    # base64 64B (kgctl key jwt)
KOOIX_DATABASE_URL  # postgres://...
KOOIX_REDIS_URL     # redis://...
KOOIX_PUBLIC_URL    # https://gate.example.com — OIDC redirect_uri 基底
```

---

## 8. 待办路线

- [x] Workspace + 领域类型 + 完整 schema
- [x] gate-crypto envelope encryption + AAD 类型化助手
- [x] gate-auth: password / JWT / API key / OIDC / AuthContext / require!
- [x] kgctl: 部署密钥生成 + env 清单
- [x] SSO schema (identity_providers + user_identities + oidc_login_states)
- [ ] gate-storage Repo 实现（users / orgs / projects / memberships / api_keys）
- [ ] gate-cache: Redis 滑动窗口 Lua + 配额扣减 Lua
- [ ] gate-server: Axum AppState + Auth 抽取器 + 控制台路由
- [ ] gate-providers: OpenAI 透传（验证 trait）
- [ ] gate-billing: usage outbox 消费者
- [ ] web: SvelteKit 控制台
