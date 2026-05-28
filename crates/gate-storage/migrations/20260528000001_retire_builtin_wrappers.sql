-- ============================================================================
-- v0.5.0-rc2 · ADR-0004 4 大编译期 wrapper 退役
-- ============================================================================
--
-- 把 openai / anthropic / azure / bedrock 4 个保留作 fast-path 的编译期 wrapper
-- 的 channels 行自动迁移到 runtime plugin preset。配合 ADR-0002 builtin_fastpath
-- 静态分发，性能无损（× 0.86-1.02 vs 旧编译期 wrapper）。
--
--   openai    → plugin + preset.provider = openai
--   anthropic → plugin + preset.provider = anthropic_messages
--   azure     → plugin + preset.provider = azure_openai
--   bedrock   → plugin + preset.provider = bedrock_converse
--
-- 迁移策略：
--   1. 在 model_mapping JSONB 中写入 `plugin.version=1` + `plugin.preset.provider`
--      （保留原 model 映射）
--   2. 把 provider_type 改为 'plugin'（统一 dispatch path）
--   3. plugin_manifest::apply_preset 在 runtime 自动注入 builtin_fastpath=true
--      + 自动切换 auth.strategy（azure→ApiKeyHeader[api-key]，anthropic→
--      ApiKeyHeader[x-api-key]，bedrock→AwsSigv4）
--   4. 幂等：再跑一次不会重复写入
--
-- ⚠ Breaking change（bedrock）：
--   旧路径：access_key = channel_keys.key_enc，secret_key = KOOIX_CH_<CODE>_SECRET env
--   新路径：access_key = secret_slot 'aws_access_key'，secret_key = secret_slot 'aws_secret_key'
--   减痛：fastpath 加 env 兜底（KOOIX_CH_<CODE>_ACCESS_KEY / KOOIX_CH_<CODE>_SECRET_KEY）
--
-- 回滚（如必须，且需同时回 0.4.x binary，0.5.x 已删 wrapper 代码）：
--   UPDATE channels
--   SET provider_type = model_mapping->'plugin'->'preset'->>'provider',
--       model_mapping = model_mapping - 'plugin'
--   WHERE provider_type = 'plugin'
--     AND model_mapping->'plugin'->'preset'->>'provider' IN
--         ('openai', 'anthropic_messages', 'azure_openai', 'bedrock_converse');
--   -- 然后把 'anthropic_messages' / 'azure_openai' / 'bedrock_converse'
--   -- 手工还原成 'anthropic' / 'azure' / 'bedrock'。
-- ============================================================================

UPDATE channels
SET model_mapping = jsonb_set(
        jsonb_set(
            jsonb_set(
                COALESCE(model_mapping, '{}'::jsonb),
                '{plugin}',
                COALESCE(model_mapping->'plugin', '{}'::jsonb),
                true
            ),
            '{plugin,version}',
            to_jsonb(1),
            true
        ),
        '{plugin,preset}',
        jsonb_build_object(
            'provider',
            CASE provider_type
                WHEN 'openai'    THEN 'openai'
                WHEN 'anthropic' THEN 'anthropic_messages'
                WHEN 'azure'     THEN 'azure_openai'
                WHEN 'bedrock'   THEN 'bedrock_converse'
            END
        ),
        true
    ),
    provider_type = 'plugin',
    updated_at = NOW()
WHERE provider_type IN ('openai', 'anthropic', 'azure', 'bedrock')
  AND deleted_at IS NULL;

-- Sanity check：迁移后所有命中行的 plugin.preset.provider 必须命中合法 fastpath preset。
DO $$
DECLARE
    bad_count INT;
BEGIN
    SELECT COUNT(*) INTO bad_count FROM channels
    WHERE provider_type = 'plugin'
      AND model_mapping->'plugin'->'preset'->>'provider' NOT IN
          ('openai', 'anthropic_messages', 'azure_openai', 'bedrock_converse',
           'openai_compatible', 'gemini', 'cohere_chat', 'deepseek', 'mistral', 'ollama',
           'vertex_openai', 'groq', 'together', 'openrouter', 'moonshot', 'zhipu',
           'qwen', 'yi', 'vllm', 'lm_studio', 'ollama_openai', 'localai', 'xinference')
      AND model_mapping->'plugin'->'preset'->>'provider' IS NOT NULL
      AND deleted_at IS NULL;
    IF bad_count > 0 THEN
        RAISE WARNING 'kooix-gate v0.5.0-rc2 migration: % rows have unrecognised plugin.preset.provider; investigate.', bad_count;
    END IF;
END
$$;
