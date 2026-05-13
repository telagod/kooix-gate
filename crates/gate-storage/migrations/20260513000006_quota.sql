-- ============================================================================
-- Quota: 多维度配额，可挂在任意主体上
-- ============================================================================

CREATE TABLE quotas (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- 挂载点
    scope_kind      TEXT NOT NULL
                    CHECK (scope_kind IN ('platform', 'org', 'project', 'user', 'membership', 'api_key')),
    scope_id        UUID NOT NULL,                       -- 通用 ID，依赖 scope_kind 解释

    -- 维度
    dimension       TEXT NOT NULL
                    CHECK (dimension IN ('rpm', 'tpm', 'concurrent',
                                         'daily_budget_usd', 'monthly_budget_usd',
                                         'lifetime_tokens')),
    model_filter    TEXT,                                -- glob pattern，NULL = 所有模型
    limit_value     NUMERIC(20, 6) NOT NULL CHECK (limit_value >= 0),
    window_seconds  INT,                                 -- 仅对 rate 类有意义

    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    notes           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- 同一主体 + 维度 + 模型范围只能有一条
    UNIQUE (scope_kind, scope_id, dimension, model_filter)
);

CREATE INDEX quotas_scope_idx ON quotas(scope_kind, scope_id) WHERE enabled = TRUE;
CREATE INDEX quotas_dimension_idx ON quotas(dimension) WHERE enabled = TRUE;

CREATE TRIGGER quotas_updated_at BEFORE UPDATE ON quotas
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- ============================================================================
-- 计费定价表（每模型 input/output token 单价，USD per 1M tokens）
-- 这是 Provider 抽象的依据之一
-- ============================================================================
CREATE TABLE model_pricing (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id          UUID REFERENCES channels(id) ON DELETE CASCADE,  -- NULL = 全局默认
    model               TEXT NOT NULL,
    input_per_million   NUMERIC(12, 6) NOT NULL CHECK (input_per_million >= 0),
    output_per_million  NUMERIC(12, 6) NOT NULL CHECK (output_per_million >= 0),
    cached_input_per_million NUMERIC(12, 6),             -- prompt caching 折扣
    effective_from      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    effective_until     TIMESTAMPTZ,                     -- NULL = 永久
    metadata            JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX model_pricing_lookup_idx ON model_pricing(channel_id, model, effective_from DESC);
CREATE TRIGGER model_pricing_updated_at BEFORE UPDATE ON model_pricing
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
