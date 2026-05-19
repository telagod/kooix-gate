-- ============================================================================
-- P1.6 Quota policy engine: dry-run mode + lifetime budget
-- ============================================================================

ALTER TABLE quotas
    ADD COLUMN IF NOT EXISTS mode TEXT NOT NULL DEFAULT 'enforce';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'quotas_mode_check'
    ) THEN
        ALTER TABLE quotas
            ADD CONSTRAINT quotas_mode_check CHECK (mode IN ('enforce', 'dry_run'));
    END IF;
END $$;

DO $$
DECLARE
    constraint_names text[];
    constraint_name text;
BEGIN
    ALTER TABLE quotas DROP CONSTRAINT IF EXISTS quotas_dimension_check;

    SELECT COALESCE(array_agg(conname), '{}') INTO constraint_names
    FROM pg_constraint
    WHERE conrelid = 'quotas'::regclass
      AND contype = 'c'
      AND conname <> 'quotas_dimension_check'
      AND pg_get_constraintdef(oid) LIKE '%dimension%';

    FOREACH constraint_name IN ARRAY constraint_names LOOP
        EXECUTE format('ALTER TABLE quotas DROP CONSTRAINT %I', constraint_name);
    END LOOP;

    ALTER TABLE quotas
        ADD CONSTRAINT quotas_dimension_check
        CHECK (dimension IN ('rpm', 'tpm', 'concurrent',
                             'daily_budget_usd', 'monthly_budget_usd', 'lifetime_budget_usd',
                             'lifetime_tokens'));
END $$;

COMMENT ON COLUMN quotas.mode IS 'enforce blocks/debits; dry_run only records would-deny signals';
