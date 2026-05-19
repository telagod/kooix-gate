-- Outbox worker lease columns for multi-worker safe consumption.
-- Rows are claimed with FOR UPDATE SKIP LOCKED and a short lease; crashed
-- workers release rows automatically when locked_until expires.

ALTER TABLE outbox_events ADD COLUMN IF NOT EXISTS locked_until TIMESTAMPTZ;
ALTER TABLE outbox_events ADD COLUMN IF NOT EXISTS locked_by TEXT;

CREATE INDEX IF NOT EXISTS outbox_events_claimable_idx
    ON outbox_events(created_at)
    WHERE processed_at IS NULL AND retry_count < 3;
