-- Seed pricing data is no longer hardcoded.
-- Global defaults are auto-synced from LiteLLM at server startup and every 24h.
-- This migration is intentionally empty (replaces previous hardcoded seed).
-- Manual overrides: INSERT into pricing_rules with channel_id set to a specific channel.
SELECT 1;
