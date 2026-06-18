-- ============================================================================
-- v0.5.0 · M5.1 N1.1 — Channel Health Score (ADR-0007)
-- ============================================================================
--
-- 号池中台护城河第一刀：4 维加权评分 + 5 状态机 + 自动 cooldown + 路由消费。
--
-- 设计稿：docs/architecture/decisions/ADR-0007-channel-health-score.md
--
-- 兼容策略：
--   - 新表 channel_health_score，1:1 跟 channels.id 关联
--   - channel_groups 加 use_health_score (BOOL, default FALSE) opt-in flag
--   - 旧 channels.health (TEXT) 字段保留作 derived view，v0.7.0 删除
--   - 路由代码 N1.4 阶段才消费 score；N1.1 只落 schema + repo
--
-- 幂等：再跑一次不会重复 insert，state CHECK 不会冲突
-- 回滚：下方 -- DOWN 段给出反向操作（手工执行）

-- ----------------------------------------------------------------------------
-- channel_health_score：每个 channel 一行
-- ----------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS channel_health_score (
    channel_id           UUID PRIMARY KEY REFERENCES channels(id) ON DELETE CASCADE,

    -- ----- 评分维度（4 维，全部归一 0-1） -----
    -- 综合加权分；越大越健康。默认 1.0 代表「新建/未观测」乐观初值。
    score                DOUBLE PRECISION NOT NULL DEFAULT 1.0
        CHECK (score >= 0.0 AND score <= 1.0),
    -- 滚动窗口的成功率
    success_rate         DOUBLE PRECISION NOT NULL DEFAULT 1.0
        CHECK (success_rate >= 0.0 AND success_rate <= 1.0),
    -- p99 延迟（毫秒），用于路由 least_latency 加权
    latency_p99_ms       INTEGER NOT NULL DEFAULT 0
        CHECK (latency_p99_ms >= 0),
    -- 封号信号（0=无信号，1=已命中检测器）。N1.3 阶段引入 BannedPatternMatcher
    banned_signal        DOUBLE PRECISION NOT NULL DEFAULT 0.0
        CHECK (banned_signal >= 0.0 AND banned_signal <= 1.0),
    -- quota 剩余归一（balance / balance_threshold）
    quota_remaining_norm DOUBLE PRECISION NOT NULL DEFAULT 1.0
        CHECK (quota_remaining_norm >= 0.0 AND quota_remaining_norm <= 1.0),
    -- 连续失败计数（成功时清零）
    consecutive_failures INTEGER NOT NULL DEFAULT 0
        CHECK (consecutive_failures >= 0),

    -- ----- 5 状态机 -----
    state                TEXT NOT NULL DEFAULT 'healthy'
        CHECK (state IN ('healthy', 'degraded', 'cooldown', 'banned', 'recovering')),
    -- 当 state = 'cooldown' 或 'recovering' 时才有意义
    cooldown_until       TIMESTAMPTZ,
    -- Banned 状态时填封号原因（来自 BannedPatternMatcher 或人工标注）
    banned_reason        TEXT,
    last_transition_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- ----- 滚动统计窗口 -----
    window_total         INTEGER NOT NULL DEFAULT 0
        CHECK (window_total >= 0),
    window_success       INTEGER NOT NULL DEFAULT 0
        CHECK (window_success >= 0 AND window_success <= window_total),
    window_started_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 路由热路径要按 state 过滤掉 cooldown/banned，加部分索引
CREATE INDEX IF NOT EXISTS channel_health_score_active_state_idx
    ON channel_health_score(state, cooldown_until)
    WHERE state IN ('cooldown', 'recovering');

-- 健康仪表盘按 score 排序时走索引
CREATE INDEX IF NOT EXISTS channel_health_score_score_idx
    ON channel_health_score(score);

-- ----------------------------------------------------------------------------
-- channel_groups 扩字段：opt-in flag + per-group 权重覆写
-- ----------------------------------------------------------------------------

ALTER TABLE channel_groups
    ADD COLUMN IF NOT EXISTS use_health_score BOOLEAN NOT NULL DEFAULT FALSE;

-- per-group 权重覆写，NULL = 用全局默认（0.4/0.3/0.2/0.1）
-- 结构示例: {"success_rate":0.5,"latency_p99":0.3,"banned_signal":0.1,"quota_remaining":0.1}
ALTER TABLE channel_groups
    ADD COLUMN IF NOT EXISTS health_weights JSONB;

-- ----------------------------------------------------------------------------
-- 存量数据初始化（幂等）
-- ----------------------------------------------------------------------------

INSERT INTO channel_health_score (channel_id)
SELECT id FROM channels
ON CONFLICT (channel_id) DO NOTHING;

-- ============================================================================
-- DOWN（手工执行回滚）
-- ============================================================================
--   ALTER TABLE channel_groups DROP COLUMN IF EXISTS health_weights;
--   ALTER TABLE channel_groups DROP COLUMN IF EXISTS use_health_score;
--   DROP INDEX IF EXISTS channel_health_score_score_idx;
--   DROP INDEX IF EXISTS channel_health_score_active_state_idx;
--   DROP TABLE IF EXISTS channel_health_score;
