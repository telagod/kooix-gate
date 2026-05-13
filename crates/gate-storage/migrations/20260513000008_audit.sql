-- ============================================================================
-- Audit Logs: 所有配置变更必落
-- ============================================================================

CREATE TABLE audit_logs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    ts              TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- 行为人
    actor_kind      TEXT NOT NULL CHECK (actor_kind IN ('user', 'api_key', 'system')),
    actor_id        UUID,                                -- user_id 或 api_key_id
    actor_ip        INET,
    actor_user_agent TEXT,
    request_id      UUID,                                -- 关联请求 trace

    -- 行为
    action          TEXT NOT NULL,                       -- 'apikey.create', 'quota.update' 等
    resource_kind   TEXT NOT NULL,
    resource_id     UUID,

    -- 上下文（多 Org 隔离用）
    org_id          UUID,
    project_id      UUID,

    -- 变更内容
    before          JSONB,
    after           JSONB,

    -- 结果
    outcome         TEXT NOT NULL DEFAULT 'success'
                    CHECK (outcome IN ('success', 'failure', 'denied')),
    error_message   TEXT
);

CREATE INDEX audit_logs_ts_idx ON audit_logs(ts DESC);
CREATE INDEX audit_logs_actor_idx ON audit_logs(actor_kind, actor_id, ts DESC);
CREATE INDEX audit_logs_resource_idx ON audit_logs(resource_kind, resource_id, ts DESC);
CREATE INDEX audit_logs_org_idx ON audit_logs(org_id, ts DESC) WHERE org_id IS NOT NULL;
CREATE INDEX audit_logs_project_idx ON audit_logs(project_id, ts DESC) WHERE project_id IS NOT NULL;
CREATE INDEX audit_logs_action_idx ON audit_logs(action, ts DESC);
