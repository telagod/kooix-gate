-- ============================================================================
-- API Keys: 项目级凭证，可限定模型 / IP / 过期
-- ============================================================================

CREATE TABLE api_keys (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    key_hash        TEXT NOT NULL UNIQUE,       -- SHA-256(plaintext)
    key_prefix      TEXT NOT NULL,              -- 'sk-kg-' + first 8 chars，明文展示用
    key_last4       TEXT NOT NULL,              -- 末 4 位
    created_by      UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,

    -- 范围限制
    allowed_models  TEXT[] NOT NULL DEFAULT '{}', -- 空数组 = 不限制
    allowed_ips     CIDR[] NOT NULL DEFAULT '{}',
    allowed_groups  UUID[] NOT NULL DEFAULT '{}', -- 限定可路由的 channel_group_id

    -- 生命周期
    expires_at      TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ,
    revoked_by      UUID REFERENCES users(id),
    revoked_reason  TEXT,
    last_used_at    TIMESTAMPTZ,
    last_used_ip    INET,
    use_count       BIGINT NOT NULL DEFAULT 0,

    metadata        JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX api_keys_project_idx ON api_keys(project_id) WHERE revoked_at IS NULL;
CREATE INDEX api_keys_hash_idx ON api_keys(key_hash) WHERE revoked_at IS NULL;
CREATE INDEX api_keys_expires_idx ON api_keys(expires_at) WHERE revoked_at IS NULL AND expires_at IS NOT NULL;

CREATE TRIGGER api_keys_updated_at BEFORE UPDATE ON api_keys
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
