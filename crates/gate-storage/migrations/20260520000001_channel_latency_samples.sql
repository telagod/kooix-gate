-- Persistent latency samples for least_latency routing.
-- Keep this table compact: low-cardinality source labels and narrow indexes.

CREATE TABLE IF NOT EXISTS channel_latency_samples (
    id          BIGSERIAL PRIMARY KEY,
    channel_id  UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    ts          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    latency_ms  BIGINT NOT NULL CHECK (latency_ms >= 0),
    success     BOOLEAN NOT NULL,
    source      TEXT NOT NULL DEFAULT 'request'
                CHECK (source IN ('request', 'health_probe'))
);

CREATE INDEX IF NOT EXISTS channel_latency_samples_channel_ts_idx
    ON channel_latency_samples(channel_id, ts DESC);

CREATE INDEX IF NOT EXISTS channel_latency_samples_success_recent_idx
    ON channel_latency_samples(channel_id, ts DESC)
    WHERE success;
