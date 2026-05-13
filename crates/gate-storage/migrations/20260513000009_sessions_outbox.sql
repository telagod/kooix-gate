-- ============================================================================
-- 会话 & Refresh Token（控制台登录用，与 API Key 区分）
-- ============================================================================

CREATE TABLE user_sessions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    refresh_token_hash TEXT NOT NULL UNIQUE,
    user_agent      TEXT,
    ip              INET,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at      TIMESTAMPTZ NOT NULL,
    revoked_at      TIMESTAMPTZ
);

CREATE INDEX user_sessions_user_idx ON user_sessions(user_id) WHERE revoked_at IS NULL;
CREATE INDEX user_sessions_expires_idx ON user_sessions(expires_at) WHERE revoked_at IS NULL;

-- ============================================================================
-- 后台任务 outbox（保证审计/计费等关键事件最终一致）
-- ============================================================================
CREATE TABLE outbox_events (
    id              BIGSERIAL PRIMARY KEY,
    topic           TEXT NOT NULL,                       -- 'usage', 'audit', 'webhook'
    payload         JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at    TIMESTAMPTZ,
    retry_count     INT NOT NULL DEFAULT 0,
    last_error      TEXT
);

CREATE INDEX outbox_events_pending_idx ON outbox_events(created_at) WHERE processed_at IS NULL;
