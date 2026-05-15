-- pricing_rules: 多模态 / 多维度定价引擎
-- 替代原 model_pricing 的 3 列模型，支持：
--   token 分类计费（input/output/cached/reasoning/audio/image）
--   图片按次（quality×size）
--   音频按字符/分钟
--   批量折扣/地域乘数/分阶上下文计费

CREATE TABLE pricing_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- NULL = 全局默认; 非 NULL = 渠道专属定价
    channel_id UUID REFERENCES channels(id) ON DELETE CASCADE,
    -- 模型名，支持通配符: "gpt-4o", "dall-e-*", "*" (全局 fallback)
    model TEXT NOT NULL,
    -- 计费维度
    dimension TEXT NOT NULL,
    -- 计费单位
    unit TEXT NOT NULL,
    -- 费率: USD per unit
    rate NUMERIC(14,8) NOT NULL CHECK (rate >= 0),
    -- 条件 (JSON 匹配): {"quality":"hd","size":"1024x1792"} 等
    conditions JSONB NOT NULL DEFAULT '{}',
    -- 时间窗口
    effective_from TIMESTAMPTZ NOT NULL DEFAULT now(),
    effective_until TIMESTAMPTZ,
    -- 同维度多条 rule 时，priority DESC 取最高
    priority INT NOT NULL DEFAULT 0,
    -- 备注
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 查询索引：(channel_id, model, dimension, effective_from DESC)
CREATE INDEX pricing_rules_lookup_idx
    ON pricing_rules (channel_id, model, dimension, effective_from DESC);

-- 全局默认快速查
CREATE INDEX pricing_rules_global_idx
    ON pricing_rules (model, dimension, effective_from DESC)
    WHERE channel_id IS NULL;

-- updated_at trigger
CREATE TRIGGER pricing_rules_updated_at
    BEFORE UPDATE ON pricing_rules
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- 迁移旧 model_pricing 数据到 pricing_rules
INSERT INTO pricing_rules (channel_id, model, dimension, unit, rate, conditions, effective_from, effective_until, priority)
SELECT
    channel_id, model, 'input_tokens', 'per_million_tokens', input_per_million, '{}'::jsonb, effective_from, effective_until, 0
FROM model_pricing
UNION ALL
SELECT
    channel_id, model, 'output_tokens', 'per_million_tokens', output_per_million, '{}'::jsonb, effective_from, effective_until, 0
FROM model_pricing
UNION ALL
SELECT
    channel_id, model, 'cached_input_tokens', 'per_million_tokens', cached_input_per_million, '{}'::jsonb, effective_from, effective_until, 0
FROM model_pricing
WHERE cached_input_per_million IS NOT NULL;

-- 保留旧表（不删除，避免破坏性），但标记为 deprecated
COMMENT ON TABLE model_pricing IS 'DEPRECATED: migrated to pricing_rules. Will be removed in future version.';
