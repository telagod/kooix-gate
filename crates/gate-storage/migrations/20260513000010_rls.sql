-- ============================================================================
-- Row-Level Security: 兜底的租户隔离
-- 应用层应该在每个请求开始时执行 SET LOCAL app.current_org_id = '...'
-- 即使应用层漏写 WHERE org_id = ?, RLS 也会兜住
-- ============================================================================

-- 启用 RLS（默认 deny，必须显式 policy 才能访问）
ALTER TABLE projects                 ENABLE ROW LEVEL SECURITY;
ALTER TABLE project_memberships      ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_keys                 ENABLE ROW LEVEL SECURITY;
ALTER TABLE model_aliases            ENABLE ROW LEVEL SECURITY;
ALTER TABLE project_group_bindings   ENABLE ROW LEVEL SECURITY;
ALTER TABLE quotas                   ENABLE ROW LEVEL SECURITY;
ALTER TABLE usage_records            ENABLE ROW LEVEL SECURITY;
ALTER TABLE inflight_requests        ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_logs               ENABLE ROW LEVEL SECURITY;

-- 角色：
--   gate_app      —— 应用连接用，受 RLS 约束
--   gate_admin    —— 平台运维 / 迁移用，BYPASSRLS
-- （生产部署时通过 GRANT 控制，此处仅给策略；角色创建可在 deploy 脚本中）

-- 工具函数：从 session 变量读取当前租户上下文
CREATE OR REPLACE FUNCTION current_org_id() RETURNS UUID AS $$
    SELECT NULLIF(current_setting('app.current_org_id', TRUE), '')::UUID;
$$ LANGUAGE SQL STABLE;

CREATE OR REPLACE FUNCTION current_project_id() RETURNS UUID AS $$
    SELECT NULLIF(current_setting('app.current_project_id', TRUE), '')::UUID;
$$ LANGUAGE SQL STABLE;

CREATE OR REPLACE FUNCTION is_platform_admin() RETURNS BOOLEAN AS $$
    SELECT COALESCE(current_setting('app.is_platform_admin', TRUE)::BOOLEAN, FALSE);
$$ LANGUAGE SQL STABLE;

-- ============================================================================
-- 策略：所有 project 范围数据 must match current_org_id / current_project_id
-- ============================================================================

CREATE POLICY projects_org_isolation ON projects
    USING (is_platform_admin() OR org_id = current_org_id());

CREATE POLICY project_memberships_isolation ON project_memberships
    USING (
        is_platform_admin()
        OR project_id IN (
            SELECT id FROM projects WHERE org_id = current_org_id()
        )
    );

CREATE POLICY api_keys_isolation ON api_keys
    USING (
        is_platform_admin()
        OR project_id IN (
            SELECT id FROM projects WHERE org_id = current_org_id()
        )
    );

CREATE POLICY model_aliases_isolation ON model_aliases
    USING (
        is_platform_admin()
        OR project_id IN (
            SELECT id FROM projects WHERE org_id = current_org_id()
        )
    );

CREATE POLICY project_group_bindings_isolation ON project_group_bindings
    USING (
        is_platform_admin()
        OR project_id IN (
            SELECT id FROM projects WHERE org_id = current_org_id()
        )
    );

CREATE POLICY quotas_isolation ON quotas
    USING (
        is_platform_admin()
        OR (
            scope_kind = 'platform'
        )
        OR (
            scope_kind = 'org' AND scope_id = current_org_id()
        )
        OR (
            scope_kind IN ('project', 'api_key', 'user', 'membership')
            AND scope_id IN (
                SELECT id FROM projects WHERE org_id = current_org_id()
                UNION
                SELECT id FROM api_keys WHERE project_id IN (
                    SELECT id FROM projects WHERE org_id = current_org_id()
                )
            )
        )
    );

CREATE POLICY usage_records_isolation ON usage_records
    USING (is_platform_admin() OR org_id = current_org_id());

CREATE POLICY inflight_requests_isolation ON inflight_requests
    USING (
        is_platform_admin()
        OR project_id IN (
            SELECT id FROM projects WHERE org_id = current_org_id()
        )
    );

CREATE POLICY audit_logs_isolation ON audit_logs
    USING (is_platform_admin() OR org_id = current_org_id() OR org_id IS NULL);
