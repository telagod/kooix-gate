-- Fix: chk_quotas_scope_id_consistency missed api_key and membership scope kinds
ALTER TABLE quotas DROP CONSTRAINT IF EXISTS chk_quotas_scope_id_consistency;
ALTER TABLE quotas ADD CONSTRAINT chk_quotas_scope_id_consistency
    CHECK (
        (scope_kind = 'org' AND scope_id IS NOT NULL)
        OR (scope_kind = 'project' AND scope_id IS NOT NULL)
        OR (scope_kind = 'user' AND scope_id IS NOT NULL)
        OR (scope_kind = 'api_key' AND scope_id IS NOT NULL)
        OR (scope_kind = 'membership' AND scope_id IS NOT NULL)
        OR (scope_kind = 'platform')
    );
