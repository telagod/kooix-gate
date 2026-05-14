-- ============================================================================
-- RLS 平台配额收紧 + scope_kind/scope_id 一致性约束
--
-- 变更：
-- 1. quotas 的 SELECT/INSERT/UPDATE/DELETE 策略要求 scope_kind = 'platform' 行
--    只允许 super_admin (is_platform_admin()) 访问
-- 2. 增加 CHECK 约束保证 scope_id 与 scope_kind 的一致性
-- ============================================================================

-- ── 1. 删除旧的 quotas RLS 策略并重建 ─────────────────────────────────────────

DROP POLICY IF EXISTS quotas_isolation ON quotas;

-- SELECT: platform 配额仅 platform admin 可见
CREATE POLICY quotas_select ON quotas FOR SELECT
    USING (
        is_platform_admin()
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

-- INSERT: platform 配额仅 platform admin 可插入
CREATE POLICY quotas_insert ON quotas FOR INSERT
    WITH CHECK (
        is_platform_admin()
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

-- UPDATE: platform 配额仅 platform admin 可修改
CREATE POLICY quotas_update ON quotas FOR UPDATE
    USING (
        is_platform_admin()
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
    )
    WITH CHECK (
        is_platform_admin()
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

-- DELETE: platform 配额仅 platform admin 可删除
CREATE POLICY quotas_delete ON quotas FOR DELETE
    USING (
        is_platform_admin()
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

-- ── 2. scope_kind 与 scope_id 一致性 CHECK ────────────────────────────────────

ALTER TABLE quotas ADD CONSTRAINT chk_quotas_scope_id_consistency
    CHECK (
        (scope_kind = 'org' AND scope_id IS NOT NULL)
        OR (scope_kind = 'project' AND scope_id IS NOT NULL)
        OR (scope_kind = 'user' AND scope_id IS NOT NULL)
        OR (scope_kind = 'platform')
    );
