-- Align strategy CHECK with Rust code:
-- schema had: weighted, priority, fallback, round_robin, least_latency
-- code uses: weighted_random, priority, round_robin, least_conn
-- Also add fallback for forward compat.

-- 1. Drop old CHECK, add new
ALTER TABLE channel_groups DROP CONSTRAINT IF EXISTS channel_groups_strategy_check;
ALTER TABLE channel_groups ADD CONSTRAINT channel_groups_strategy_check
    CHECK (strategy IN ('priority', 'weighted_random', 'round_robin', 'least_conn', 'fallback'));

-- 2. Migrate existing rows
UPDATE channel_groups SET strategy = 'weighted_random' WHERE strategy = 'weighted';
UPDATE channel_groups SET strategy = 'least_conn'      WHERE strategy = 'least_latency';
