-- Add quota recovery columns to inflight_requests for crash-safe pre-debit
ALTER TABLE inflight_requests ADD COLUMN IF NOT EXISTS quota_keys TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE inflight_requests ADD COLUMN IF NOT EXISTS estimated_micros BIGINT[] NOT NULL DEFAULT '{}';
