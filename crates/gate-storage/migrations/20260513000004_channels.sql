-- ============================================================================
-- Channels: 平台级上游连接 + Key 池（带健康熔断）
-- ============================================================================

CREATE TABLE channels (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code            CITEXT NOT NULL UNIQUE,             -- 'openai-prod', 'azure-westus' 等
    name            TEXT NOT NULL,
    provider_type   TEXT NOT NULL,                       -- 'openai' | 'anthropic' | 'gemini' | 'azure' | 'bedrock' | 'wasm:custom'
    base_url        TEXT NOT NULL,
    config_enc      BYTEA NOT NULL,                      -- 加密 JSON (aes-gcm)，含 provider 特定参数
    supported_models TEXT[] NOT NULL DEFAULT '{}',       -- 该渠道支持的模型列表
    status          TEXT NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'disabled', 'deleted')),

    -- 全局熔断状态（基于所有 key 聚合）
    health          TEXT NOT NULL DEFAULT 'healthy'
                    CHECK (health IN ('healthy', 'degraded', 'unhealthy')),
    last_error      TEXT,
    last_error_at   TIMESTAMPTZ,

    -- 默认参数
    timeout_ms      INT NOT NULL DEFAULT 60000,
    max_retries     INT NOT NULL DEFAULT 2,

    owner_admin_id  UUID REFERENCES users(id),
    metadata        JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at      TIMESTAMPTZ
);

CREATE INDEX channels_status_idx ON channels(status, health) WHERE deleted_at IS NULL;
CREATE TRIGGER channels_updated_at BEFORE UPDATE ON channels
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Key 池：一个渠道可挂 N 个 key，独立健康追踪
CREATE TABLE channel_keys (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id          UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    label               TEXT,                            -- 'primary', 'backup', 等
    key_enc             BYTEA NOT NULL,                  -- 加密的 API key
    key_fingerprint     TEXT NOT NULL,                   -- SHA-256 前 16 字节 hex，去重用
    weight              INT NOT NULL DEFAULT 1 CHECK (weight >= 0),

    -- 健康状态（核心：熔断器）
    health              TEXT NOT NULL DEFAULT 'healthy'
                        CHECK (health IN ('healthy', 'cooling_down', 'disabled')),
    consecutive_errors  INT NOT NULL DEFAULT 0,
    last_error_code     INT,                             -- HTTP code 或自定义
    last_error_at       TIMESTAMPTZ,
    cooldown_until      TIMESTAMPTZ,                     -- 冷却期结束时间

    -- 累计指标（异步更新，仅展示用）
    total_requests      BIGINT NOT NULL DEFAULT 0,
    total_errors        BIGINT NOT NULL DEFAULT 0,

    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (channel_id, key_fingerprint)
);

CREATE INDEX channel_keys_channel_idx ON channel_keys(channel_id, health);
CREATE INDEX channel_keys_cooldown_idx ON channel_keys(cooldown_until) WHERE cooldown_until IS NOT NULL;
CREATE TRIGGER channel_keys_updated_at BEFORE UPDATE ON channel_keys
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
