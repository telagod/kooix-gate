# ADR-0007: Channel Health Score —— 号池中台护城河（v0.5.0 M5.1）

- Status: **Accepted (2026-06-19)** — N1.1 schema + repo + 13 PG integration tests + 11 unit tests 全绿；N1.2-N1.6 实装中
- Deciders: telagod
- Affected: `crates/gate-storage/migrations/20260619000001_channel_health_score.sql`（新增）, `crates/gate-storage/src/repo/channel.rs`, `crates/gate-providers/src/router/{mod.rs,selection.rs,helpers.rs,builder.rs}`, `crates/gate-server/src/health_check.rs`, `crates/gate-server/src/routes/admin/channels.rs`, control plane 「号池健康仪表盘」UI
- 关联：实现 [ROADMAP M5.1](../../../ROADMAP.md#m51-channelhealthscore核心)；扩展 [ADR-0005 Native Provider Plane](./ADR-0005-native-provider-plane.md) 三档渠道分级里的「重渠道」健康判定；与 [ADR-0001 Plugin Manifest](./ADR-0001-providers-as-plugin.md) probe / health 字段对接

## Context

Kooix Gate 当前 5 种路由策略（`priority / weighted_random / round_robin / least_conn / least_latency`）只看两个维度：

- `inflight`：实时并发数（`router/mod.rs` `InflightTracker`）
- `latency`：持久化滑窗（`router/mod.rs:126` `latency` repo）

这两个维度回答的是「**当前请求该走哪条**」，**回答不了**号池中台的核心问题：

> 这号是不是要废了？

**「这号要废了」** 是号池玩家面对的真实日常：

- 上游突然返 `401 / 403` → 账号被风控
- 余额耗尽 → 上游静默返"无可用额度"
- `429 Retry-After: 86400` → 账号被限流一天，但 channel 还在被路由灌请求
- 上游返 `200 OK` + body 内含「Your account has been suspended」 → 协议层成功但语义层挂掉
- 突发 5×× 风暴 → 上游某地区/某节点挂

这些信号**当前路由完全看不见**——`health: String` 字段（`channel.rs:27`）是简单的 `healthy / degraded / down`，只在 probe 周期里被 health_check 写入，路由策略选路时**根本不读它**（grep `provider_capabilities` / `router/selection.rs` 无 `health` 字段消费）。

竞品对比这个空白：

| 系统 | 健康判定 |
|------|---------|
| OneAPI / NewAPI | 简单二元 `enabled`，没有自愈，没有评分 |
| LiteLLM | retry + fallback，没有持久化健康度 |
| OpenRouter | 闭源 SaaS，黑盒 |
| **Kooix Gate（当前）** | health 字段未参与路由 |
| **Kooix Gate（M5）** | 多维评分 + 状态机 + 自动 cooldown + 路由消费 |

**「ChannelHealthScore + 状态机 + 自动 cooldown」是号池中台 vs API 聚合的分水岭。** 这是 M5 v0.5.0 必交付的核心模块。

## Decision

引入 **`ChannelHealthScore`** —— 一个 0-1 归一的多维评分，配套 5 状态机 + 自动 cooldown + 路由策略消费链路。新表 `channel_health_score` 落库，路由热路径 in-memory cache，5 种现有策略统一接入 `health_weight` 权重。

### 1. 评分模型

`score ∈ [0.0, 1.0]`，越高越健康：

```text
score = w_succ * success_rate
      + w_lat  * (1 - normalize(latency_p99))
      + w_ban  * (1 - banned_signal)
      + w_quota* quota_remaining_norm
```

| 维度 | 默认权重 | 数据源 | 归一 |
|------|---------|--------|------|
| `success_rate` | **0.40** | 滚动 60s / 600 个请求窗口 | `success_count / total_count` |
| `latency_p99` | **0.30** | 滚动 60s p99（已有 `latency` repo） | `clamp(p99_ms / 30000, 0, 1)`，30s 视为最差 |
| `banned_signal` | **0.20** | 封号检测器（详见 §3） | 命中=1，未命中=0 |
| `quota_remaining` | **0.10** | balance / quota probe | `clamp(balance / balance_threshold, 0, 1)` |

权重默认值入 `channel_groups.health_weights JSONB`，每个 group 可覆写——例如「免费号池」对 quota_remaining 权重拉到 0.4。

> **抖动抑制**：分数变化引入 hysteresis（迟滞 0.05），避免在边界来回切。

### 2. 状态机

```
                              ┌──────────────┐
                              │              │
                              ↓              │
        score ≥ 0.7    ┌───────────┐  cooldown_expired
        ┌──────────────│  Healthy  │  + probe pass
        │              └───────────┘ ←──────────────┐
        ↓                   ↑                       │
   ┌───────────┐       score ≥ 0.7                  │
   │ Degraded  │ ────────────┘                      │
   │ 0.4-0.7   │                                    │
   └───────────┘                                    │
        │                                           │
   score < 0.4                                      │
   OR auto_cooldown                                 │
        ↓                                           │
   ┌────────────┐    cooldown_expired               │
   │  Cooldown  │ ─────────────────→ ┌─────────────┐│
   │            │                    │ Recovering  │┘
   └────────────┘                    │ (probe loop)│
        │                            └─────────────┘
   banned_signal 命中                       │
   OR 连续 5 次失败                   probe fail × 3
        ↓                                   ↓
   ┌─────────┐                       ┌──────────┐
   │ Banned  │ ←──────────────────── │  Cooldown │
   │ (终态)  │                       │  (重新)   │
   └─────────┘                       └──────────┘
```

| 状态 | 路由行为 | 退出条件 |
|------|---------|---------|
| `Healthy` | 全权重参与 | score < 0.7 → Degraded |
| `Degraded` | 权重 × 0.5（降级） | score ≥ 0.7 → Healthy；score < 0.4 → Cooldown |
| `Cooldown` | **跳过路由**（不参与） | `cooldown_until <= NOW()` → Recovering |
| `Recovering` | 只接 probe 流量（capped）| probe pass × 3 → Healthy；probe fail × 3 → Cooldown |
| `Banned` | **永久跳过**（人工解封） | 仅 admin API 可转回 Cooldown |

> `Banned` 是终态：人工介入前一律跳过。号池玩家"换号"是常规操作，自动恢复有风险。

### 3. 封号检测器（Banned Signal Detector）

封号信号 = 协议+语义+量化三层规则，命中任一即触发 `banned_signal=1`：

| 层 | 规则 | 实现位置 |
|---|------|---------|
| 协议层 | `401 / 403` 连续 N 次（默认 3） | `provider_error.rs` 统计 |
| 协议层 | `429` 带 `Retry-After > cooldown_max`（默认 1h） | manifest error mapper |
| 语义层 | `200 OK` + 响应 body 匹配 vendor-specific 封号正则 | `BannedPatternMatcher` trait，每个 preset 可注册 |
| 量化层 | `balance < balance_floor`（可配置） | `channel.balance_updated_at` 触发 |

`BannedPatternMatcher` 是 trait：

```rust
pub trait BannedPatternMatcher: Send + Sync {
    /// 解析响应判定封号。返回 `Some(reason)` 即命中。
    fn detect(&self, status: u16, body: &str, headers: &HeaderMap) -> Option<String>;
}
```

每个 provider preset 注册自己的实现——OpenAI 的「account_deactivated」、Anthropic 的「invalid_api_key 持续返」、Azure 的「DeploymentNotFound 但 endpoint 还在」都是不同 pattern。

### 4. 自动 Cooldown 算法

```text
cooldown_ms = min(
    base_cooldown * 2^consecutive_failures,
    max_cooldown
)
```

- `base_cooldown` 默认 `30s`，`max_cooldown` 默认 `30min`（可 channel-level 覆写）
- 连续失败计数器在 `Healthy` 转移时清零
- `429 Retry-After` 头存在时**用上游值**（取较大者）

### 5. 路由策略消费

5 种策略**统一**接受 `health_weight`：

| 策略 | 改动 |
|------|------|
| `priority` | `Cooldown / Banned` 跳过；同 priority 按 score 排序 |
| `weighted_random` | `effective_weight = weight × max(score, MIN_WEIGHT_FLOOR)` |
| `round_robin` | `Cooldown / Banned` 跳过；不影响轮转计数 |
| `least_conn` | 同 inflight 取 score 高者 |
| `least_latency` | `effective_latency = latency_p99 × (2 - score)`（健康度差的视为延迟更高）|

`MIN_WEIGHT_FLOOR = 0.05`：避免低分 channel 被永久饿死，留 5% 探针流量做 score 恢复探测。

### 6. Schema

新表 + ChannelRecord 扩展字段：

```sql
-- migrations/20260619000001_channel_health_score.sql

CREATE TABLE channel_health_score (
    channel_id           UUID PRIMARY KEY REFERENCES channels(id) ON DELETE CASCADE,

    -- 评分维度
    score                DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    success_rate         DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    latency_p99_ms       INTEGER NOT NULL DEFAULT 0,
    banned_signal        DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    quota_remaining_norm DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,

    -- 状态机
    state                TEXT NOT NULL DEFAULT 'healthy'
        CHECK (state IN ('healthy','degraded','cooldown','banned','recovering')),
    cooldown_until       TIMESTAMPTZ,
    banned_reason        TEXT,
    last_transition_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- 统计窗口
    window_total         INTEGER NOT NULL DEFAULT 0,
    window_success       INTEGER NOT NULL DEFAULT 0,
    window_started_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX channel_health_score_state_idx
    ON channel_health_score(state)
    WHERE state IN ('cooldown','recovering');

-- channel_groups 加 health_weights 覆写
ALTER TABLE channel_groups
    ADD COLUMN health_weights JSONB,
    ADD COLUMN use_health_score BOOLEAN NOT NULL DEFAULT FALSE;

-- 为存量数据初始化
INSERT INTO channel_health_score (channel_id)
SELECT id FROM channels
ON CONFLICT DO NOTHING;
```

`ChannelRecord` 扩展（不删 `health: String`，作为派生视图保留）：

```rust
pub struct ChannelRecord {
    // ... 现有字段 ...
    pub health: String,  // 兼容字段：derived from state，留作 v0.5 → v0.6 兼容窗口
    pub health_score: Option<ChannelHealthScore>,  // 新增，None = 未启用
}

pub struct ChannelHealthScore {
    pub score: f64,
    pub state: HealthState,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub banned_reason: Option<String>,
    pub success_rate: f64,
    pub latency_p99_ms: i32,
    pub consecutive_failures: i32,
}

pub enum HealthState { Healthy, Degraded, Cooldown, Recovering, Banned }
```

### 7. 数据流

```
Request → Router::route()
        → ScoreCache (in-memory, TTL 1s)
            ↓ miss
            channel_health_score table read
        → strategy::pick() consumes score + state
        → upstream call
        → record_outcome(success / failure / banned_signal)
            ↓ batched
            ScoreUpdater::flush_async() (every 1s OR 100 events)
            → channel_health_score table write
            → state transition triggered if threshold crossed
        → response
```

**热路径开销 = 1 次 in-memory cache 读**。Score 更新走 async batch flush，**不阻塞请求路径**。

### 8. Metric

```
gate_channel_health_score{channel_id, state}     gauge 0-1
gate_channel_state                                gauge (enum int)
gate_channel_state_transitions_total{from,to}     counter
gate_channel_cooldown_total{reason}               counter
gate_channel_banned_total{reason}                 counter
gate_channel_routing_skipped_total{state}         counter
```

### 9. 兼容窗口

- **v0.5.0**：opt-in。`channel_groups.use_health_score = false` 默认，行为与当前一致
- **v0.5.x**：admin UI 可逐 group 启用，监控 + 比对
- **v0.6.0**：默认 `use_health_score = true`，旧字段 `health: String` 标 `#[deprecated]`
- **v0.7.0**：删除 `health: String`，强制走 score

### 10. Admin API

```text
GET    /v1/admin/channels/:id/health-score      获取实时评分 + 状态
POST   /v1/admin/channels/:id/health/unban      人工解封（Banned → Cooldown）
POST   /v1/admin/channels/:id/health/cooldown   人工强制 cooldown（管理员）
GET    /v1/admin/groups/:id/health-weights      查看 group 权重覆写
PUT    /v1/admin/groups/:id/health-weights      更新 group 权重覆写
GET    /v1/admin/health-dashboard                号池健康仪表盘聚合视图
```

### 11. UI

「**号池健康仪表盘**」`/admin/health-dashboard`：

- 全局：当前 5 状态各占比（pie）+ 历史状态转移率（line）
- 渠道矩阵：channel × score × state × cooldown_until × banned_reason × 最近 5 次 transition
- 告警列表：进入 `Banned` 状态的 channel + 长期 `Cooldown` 未恢复 + 频繁抖动的

## Consequences

### 正向

- ✅ **号池中台护城河立**——OneAPI/LiteLLM 都没有的能力
- ✅ **封号自愈**——账号被风控 → 自动 cooldown → 不再灌请求 → 自动 probe 恢复
- ✅ **统一 5 路由策略**：所有策略都按 score 修正，不用为每条策略单独写规则
- ✅ **可配置**：每 group 可自定义权重，免费号池 vs 付费号池行为可不同
- ✅ **可视化**：「号池健康仪表盘」是产品级 UI，号池运维直接受益
- ✅ **opt-in 安全**：v0.5.0 默认关闭，主线无回归风险

### 反向

- ⚠ **新数据表 + 新写入路径**：增加 PG 写压力，要 batched flush 缓解
- ⚠ **状态机抖动**：评分边界来回切的风险——通过 hysteresis + 状态保持期缓解
- ⚠ **封号 pattern 维护**：每加一个 preset 要注册 `BannedPatternMatcher` 实现；不维护就降级到协议层规则
- ⚠ **学习曲线**：5 状态机 + 4 维评分 + 权重覆写——admin UI 必须直观

### 不在本 ADR 范围

下列功能属于 M5 后续子任务，不在 ADR-0007 范围：

- **余额监控的具体协议探测**（M5.2 N2.2）：每家上游 balance probe 路径不同，留 `channel_health_score.quota_probe_path` 字段后续填
- **批量准入**（M5.2 N2.4）：CSV 导入 N 个账号 + 自动 probe + 初始 score
- **反指纹 / Cookie Session 管理**（M7）：Native provider 侧的「难接入」健康判定
- **跨地区路由**（v0.6+）：region-aware 健康度

## Verification

发版前必过：

```bash
# 单元：score 计算 + 状态机转移
cargo test -p gate-providers health_score
cargo test -p gate-storage channel_health_score_repo

# 路由集成：strategy × state 矩阵
cargo test -p gate-providers --test routing_with_health

# Migration：迁移 + 回滚 + 存量初始化幂等
cargo test -p gate-storage --test pg_repo health_score_migration

# E2E：人工注入 401 → 自动 cooldown → cooldown_until 到点 → recovering → probe pass → healthy
cargo test -p gate-server --test channel_health_e2e

# Bench：路由热路径 p99 增量 ≤ +5%
cargo bench --package gate-providers --bench routing_with_health
```

发版门禁：

- [ ] Migration 20260619000001 在空库 + v0.5.0-rc2 库都跑通
- [ ] `channel_groups.use_health_score = false` 时行为与 v0.5.0-rc2 完全一致（regression suite）
- [ ] 5 状态机所有 transition 都有单测覆盖
- [ ] 5 路由策略 × 5 状态有矩阵测试覆盖
- [ ] Bench 数据贴 README：纯路由 vs 路由 + health score 的 p99 延迟差
- [ ] Admin UI「号池健康仪表盘」可定位每个 channel 的 score 来源（4 维归一值 + 权重）

## Implementation Plan

```text
N1.1  Schema + Repo                3d
      └─ migration 20260619000001
      └─ ChannelHealthScoreRepo
      └─ HealthState enum + ChannelHealthScore struct

N1.2  评分引擎 + 状态机             5d
      └─ ScoreCalculator (4 维 → score)
      └─ StateMachine (5 状态转移)
      └─ Hysteresis + window rolling

N1.3  封号检测器                    3d
      └─ BannedPatternMatcher trait
      └─ OpenAI / Anthropic / Azure / Bedrock 实现
      └─ 协议层 + 语义层 + 量化层规则

N1.4  路由策略接入                  4d
      └─ priority / weighted_random / round_robin / least_conn / least_latency
      └─ MIN_WEIGHT_FLOOR + probe 流量留口
      └─ Cooldown / Banned skip 逻辑

N1.5  Score 异步落库                3d
      └─ ScoreUpdater + batch flush
      └─ ScoreCache (in-memory TTL)
      └─ Outcome record (request 完成时调用)

N1.6  Admin API + UI                5d
      └─ 6 个 admin endpoint
      └─ 「号池健康仪表盘」/admin/health-dashboard
      └─ DataTable + 5 状态可视化

N1.x  Bench + 文档 + runbook        2d
      └─ Criterion bench 入 CI
      └─ docs/health-score-runbook.md
      └─ README 性能数字更新

总计：~25 工作日
```

## References

- 实现 [ROADMAP M5.1](../../../ROADMAP.md#m51-channelhealthscore核心)
- 扩展 [ADR-0005 三档渠道分级](./ADR-0005-native-provider-plane.md) 的健康判定
- 与 [ADR-0001 Plugin Manifest](./ADR-0001-providers-as-plugin.md) probe 字段对接（probe 结果是评分输入之一）
- 后续 ABI v1（[ADR-0006](./ADR-0006-wasm-abi-v1-component-model.md)）可让 WASM 插件直接消费 score 做请求路由决策
