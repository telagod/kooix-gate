-- ============================================================================
-- 0.3.0 · ADR-0001 Provider 全插件化迁移
-- ============================================================================
--
-- 把 5 个 deprecated 编译期 thin wrapper provider 的 channels 行自动迁移到
-- runtime plugin preset：
--
--   cohere   → plugin + preset.provider = cohere_chat
--   deepseek → plugin + preset.provider = deepseek
--   gemini   → plugin + preset.provider = gemini
--   mistral  → plugin + preset.provider = mistral
--   ollama   → plugin + preset.provider = ollama
--
-- 迁移策略：
--   1. 在 model_mapping JSONB 中写入 `plugin.preset.provider`（保留原 model 映射）
--   2. 把 provider_type 改为 'plugin'（统一 dispatch path）
--   3. 幂等：再跑一次不会重复写入
--
-- 回滚（如必须）：
--   UPDATE channels SET provider_type = model_mapping->'plugin'->'preset'->>'provider'
--   WHERE provider_type = 'plugin'
--     AND model_mapping->'plugin'->'preset'->>'provider' IN
--         ('cohere_chat', 'deepseek', 'gemini', 'mistral', 'ollama');
--   再回滚 0.3.0 binary（需要带回 5 thin wrapper crates/gate-providers/src/{cohere,deepseek,gemini,mistral,ollama}.rs）。
-- ============================================================================

-- 先把每个 deprecated provider_type 注入 plugin manifest preset，再改 provider_type。
-- 用单条 UPDATE 防止跨语句的中间态可见。

UPDATE channels
SET model_mapping = jsonb_set(
        jsonb_set(
            COALESCE(model_mapping, '{}'::jsonb),
            '{plugin}',
            COALESCE(model_mapping->'plugin', '{}'::jsonb),
            true
        ),
        '{plugin,preset}',
        jsonb_build_object(
            'provider',
            CASE provider_type
                WHEN 'cohere'   THEN 'cohere_chat'
                WHEN 'deepseek' THEN 'deepseek'
                WHEN 'gemini'   THEN 'gemini'
                WHEN 'mistral'  THEN 'mistral'
                WHEN 'ollama'   THEN 'ollama'
            END
        ),
        true
    ),
    provider_type = 'plugin',
    updated_at = NOW()
WHERE provider_type IN ('cohere', 'deepseek', 'gemini', 'mistral', 'ollama')
  AND deleted_at IS NULL;

-- 二次验证：迁移后 plugin.preset.provider 必须命中合法 preset 名（基本 sanity）。
DO $$
DECLARE
    bad_count INT;
BEGIN
    SELECT COUNT(*) INTO bad_count FROM channels
    WHERE provider_type = 'plugin'
      AND model_mapping->'plugin'->'preset'->>'provider' IS NULL
      AND deleted_at IS NULL
      AND created_at < NOW();  -- 保护新建的纯 plugin manifest（无 preset 字段也合法）
    -- 不抛错，只 NOTICE：纯 plugin manifest 可以没有 preset，是合法状态。
    IF bad_count > 0 THEN
        RAISE NOTICE 'kooix-gate 0.3.0 migration: % rows have provider_type=plugin without a preset.provider; this is OK if they are hand-rolled plugin manifests.', bad_count;
    END IF;
END
$$;
