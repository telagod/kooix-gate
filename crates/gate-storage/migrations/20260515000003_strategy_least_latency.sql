ALTER TABLE channel_groups DROP CONSTRAINT IF EXISTS channel_groups_strategy_check;
ALTER TABLE channel_groups ADD CONSTRAINT channel_groups_strategy_check
    CHECK (strategy IN ('priority', 'weighted_random', 'round_robin', 'least_conn', 'fallback', 'least_latency'));
