-- ============================================================================
-- Integrity Fixes: FK constraints, unique constraint, composite index, CHECK
-- ============================================================================

-- ============================================================================
-- 1. Foreign keys on usage_records
--    project_id → projects(id)
--    api_key_id → api_keys(id)
--    channel_id → channels(id)   (nullable since 20260513000012)
--    channel_key_id → channel_keys(id) (nullable)
-- ============================================================================
DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'usage_records_project_id_fkey'
    ) THEN
        ALTER TABLE usage_records
            ADD CONSTRAINT usage_records_project_id_fkey
            FOREIGN KEY (project_id) REFERENCES projects(id);
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'usage_records_api_key_id_fkey'
    ) THEN
        ALTER TABLE usage_records
            ADD CONSTRAINT usage_records_api_key_id_fkey
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id);
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'usage_records_channel_id_fkey'
    ) THEN
        ALTER TABLE usage_records
            ADD CONSTRAINT usage_records_channel_id_fkey
            FOREIGN KEY (channel_id) REFERENCES channels(id);
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'usage_records_channel_key_id_fkey'
    ) THEN
        ALTER TABLE usage_records
            ADD CONSTRAINT usage_records_channel_key_id_fkey
            FOREIGN KEY (channel_key_id) REFERENCES channel_keys(id);
    END IF;
END $$;

-- ============================================================================
-- 2. Foreign keys on inflight_requests
--    First make channel_id nullable, then add FK constraints.
-- ============================================================================
ALTER TABLE inflight_requests ALTER COLUMN channel_id DROP NOT NULL;

DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'inflight_requests_project_id_fkey'
    ) THEN
        ALTER TABLE inflight_requests
            ADD CONSTRAINT inflight_requests_project_id_fkey
            FOREIGN KEY (project_id) REFERENCES projects(id);
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'inflight_requests_api_key_id_fkey'
    ) THEN
        ALTER TABLE inflight_requests
            ADD CONSTRAINT inflight_requests_api_key_id_fkey
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id);
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'inflight_requests_channel_id_fkey'
    ) THEN
        ALTER TABLE inflight_requests
            ADD CONSTRAINT inflight_requests_channel_id_fkey
            FOREIGN KEY (channel_id) REFERENCES channels(id);
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'inflight_requests_channel_key_id_fkey'
    ) THEN
        ALTER TABLE inflight_requests
            ADD CONSTRAINT inflight_requests_channel_key_id_fkey
            FOREIGN KEY (channel_key_id) REFERENCES channel_keys(id);
    END IF;
END $$;

-- ============================================================================
-- 3. Unique constraint on model_pricing (channel_id, model, effective_from)
--    Prevents duplicate pricing rows for the same channel+model+effective_from.
-- ============================================================================
DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'model_pricing_channel_model_effective_uq'
    ) THEN
        ALTER TABLE model_pricing
            ADD CONSTRAINT model_pricing_channel_model_effective_uq
            UNIQUE (channel_id, model, effective_from);
    END IF;
END $$;

-- ============================================================================
-- 4. Composite index on quotas for fast lookup
-- ============================================================================
CREATE INDEX IF NOT EXISTS idx_quotas_lookup
    ON quotas (scope_kind, scope_id, dimension, model_filter);

-- ============================================================================
-- 5. Balance column CHECK on channels
--    balance is DOUBLE PRECISION, nullable (NULL = unknown/unsupported).
-- ============================================================================
DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'channels_balance_non_negative'
    ) THEN
        ALTER TABLE channels
            ADD CONSTRAINT channels_balance_non_negative
            CHECK (balance IS NULL OR balance >= 0);
    END IF;
END $$;
