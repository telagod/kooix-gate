-- Channel group binding canary gate.
--
-- NULL means normal binding. Non-NULL means the binding is eligible only for
-- that percentage of route attempts, expressed in basis points:
--   100  = 1%
--   500  = 5%
--   10000 = 100%

ALTER TABLE channel_group_bindings
    ADD COLUMN IF NOT EXISTS canary_percent_bps INT;

ALTER TABLE channel_group_bindings
    DROP CONSTRAINT IF EXISTS channel_group_bindings_canary_percent_bps_check;

ALTER TABLE channel_group_bindings
    ADD CONSTRAINT channel_group_bindings_canary_percent_bps_check
    CHECK (
        canary_percent_bps IS NULL
        OR (canary_percent_bps >= 0 AND canary_percent_bps <= 10000)
    );

CREATE INDEX IF NOT EXISTS channel_group_bindings_canary_idx
    ON channel_group_bindings (group_id, canary_percent_bps)
    WHERE canary_percent_bps IS NOT NULL;
