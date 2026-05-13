-- ============================================================================
-- Routing: ChannelGroup + Project 绑定 + ModelAlias
-- ============================================================================

-- 渠道分组：一组渠道 + 路由策略
CREATE TABLE channel_groups (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                TEXT NOT NULL UNIQUE,
    description         TEXT,
    strategy            TEXT NOT NULL DEFAULT 'weighted'
                        CHECK (strategy IN ('weighted', 'priority', 'fallback', 'round_robin', 'least_latency')),
    fallback_group_id   UUID REFERENCES channel_groups(id),  -- 整组失败时跳转
    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    metadata            JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TRIGGER channel_groups_updated_at BEFORE UPDATE ON channel_groups
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- 现在可以补 projects.default_group_id 的外键
ALTER TABLE projects
    ADD CONSTRAINT projects_default_group_fk
    FOREIGN KEY (default_group_id) REFERENCES channel_groups(id) ON DELETE SET NULL;

-- Group 内绑定哪些 Channel
CREATE TABLE channel_group_bindings (
    group_id        UUID NOT NULL REFERENCES channel_groups(id) ON DELETE CASCADE,
    channel_id      UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    priority        INT NOT NULL DEFAULT 100,           -- 数字越小优先级越高
    weight          INT NOT NULL DEFAULT 1 CHECK (weight >= 0),
    model_filter    TEXT[] NOT NULL DEFAULT '{}',       -- 仅这些模型走该 channel
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (group_id, channel_id)
);

CREATE INDEX channel_group_bindings_group_idx ON channel_group_bindings(group_id, enabled);

-- Project 绑定可用的 ChannelGroup（M:N）
CREATE TABLE project_group_bindings (
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    group_id        UUID NOT NULL REFERENCES channel_groups(id) ON DELETE CASCADE,
    model_pattern   TEXT,                                -- 仅匹配此 pattern 的模型走该 group
    priority        INT NOT NULL DEFAULT 100,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, group_id)
);

CREATE INDEX project_group_bindings_project_idx ON project_group_bindings(project_id, enabled);

-- 模型别名（Project 内生效）
CREATE TABLE model_aliases (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    alias           TEXT NOT NULL,                       -- 用户请求时用的名字，如 'gpt-4'
    target_model    TEXT NOT NULL,                       -- 实际模型，如 'gpt-4o-2024-08-06'
    group_id        UUID REFERENCES channel_groups(id),  -- 强制路由到该 group
    params_override JSONB NOT NULL DEFAULT '{}'::JSONB,  -- temperature/top_p 等默认值
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (project_id, alias)
);

CREATE INDEX model_aliases_project_idx ON model_aliases(project_id, enabled);
CREATE TRIGGER model_aliases_updated_at BEFORE UPDATE ON model_aliases
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
