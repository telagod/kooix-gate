-- Request event read model + rollups + immutable billing ledger.
-- usage_records remains the compatibility analytics projection; new writers dual-write
-- these narrower tables so control-plane dashboards stop depending on the hot wide table.

CREATE TABLE IF NOT EXISTS request_events (
    ts                  TIMESTAMPTZ NOT NULL,
    request_id          UUID NOT NULL,
    idempotency_key     TEXT NOT NULL,

    org_id              UUID NOT NULL,
    project_id          UUID NOT NULL,
    api_key_id          UUID NOT NULL,
    user_id             UUID,

    channel_id          UUID,
    channel_key_id      UUID,
    group_id            UUID,

    model_requested     TEXT NOT NULL,
    model_actual        TEXT NOT NULL,
    stream              BOOLEAN NOT NULL DEFAULT FALSE,

    tokens_in           INT NOT NULL DEFAULT 0,
    tokens_out          INT NOT NULL DEFAULT 0,
    tokens_cached       INT NOT NULL DEFAULT 0,
    cost_micros         BIGINT NOT NULL DEFAULT 0,
    cost_usd            NUMERIC(12, 8) NOT NULL DEFAULT 0,

    latency_ms          INT,
    ttfb_ms             INT,
    status              SMALLINT NOT NULL,
    error_code          TEXT,
    retries             SMALLINT NOT NULL DEFAULT 0,

    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    PRIMARY KEY (ts, request_id),
    UNIQUE (request_id),
    UNIQUE (idempotency_key)
);

CREATE INDEX IF NOT EXISTS request_events_request_id_idx ON request_events(request_id);
CREATE INDEX IF NOT EXISTS request_events_org_ts_idx ON request_events(org_id, ts DESC);
CREATE INDEX IF NOT EXISTS request_events_project_ts_idx ON request_events(project_id, ts DESC);
CREATE INDEX IF NOT EXISTS request_events_api_key_ts_idx ON request_events(api_key_id, ts DESC);
CREATE INDEX IF NOT EXISTS request_events_channel_ts_idx ON request_events(channel_id, ts DESC) WHERE channel_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS request_events_model_ts_idx ON request_events(model_actual, ts DESC);
CREATE INDEX IF NOT EXISTS request_events_status_ts_idx ON request_events(status, ts DESC);

CREATE TABLE IF NOT EXISTS request_event_details (
    request_id          UUID PRIMARY KEY REFERENCES request_events(request_id) ON DELETE CASCADE,
    ts                  TIMESTAMPTZ NOT NULL,
    provider_request_id TEXT,
    client_ip           INET,
    metadata            JSONB,
    error_payload       JSONB,
    trace               JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS request_event_details_ts_idx ON request_event_details(ts DESC);

CREATE TABLE IF NOT EXISTS usage_hourly_rollups (
    bucket              TIMESTAMPTZ NOT NULL,
    channel_key         UUID NOT NULL,
    org_id              UUID NOT NULL,
    project_id          UUID,
    api_key_id          UUID,
    channel_id          UUID,
    model_actual        TEXT NOT NULL,
    status_class        SMALLINT NOT NULL,
    request_count       BIGINT NOT NULL DEFAULT 0,
    error_count         BIGINT NOT NULL DEFAULT 0,
    tokens_in           BIGINT NOT NULL DEFAULT 0,
    tokens_out          BIGINT NOT NULL DEFAULT 0,
    tokens_cached       BIGINT NOT NULL DEFAULT 0,
    cost_micros         BIGINT NOT NULL DEFAULT 0,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (bucket, org_id, project_id, api_key_id, channel_key, model_actual, status_class)
);

CREATE INDEX IF NOT EXISTS usage_hourly_rollups_org_bucket_idx ON usage_hourly_rollups(org_id, bucket DESC);
CREATE INDEX IF NOT EXISTS usage_hourly_rollups_bucket_idx ON usage_hourly_rollups(bucket DESC);

CREATE TABLE IF NOT EXISTS usage_daily_rollups (
    bucket              DATE NOT NULL,
    channel_key         UUID NOT NULL,
    org_id              UUID NOT NULL,
    project_id          UUID,
    api_key_id          UUID,
    channel_id          UUID,
    model_actual        TEXT NOT NULL,
    status_class        SMALLINT NOT NULL,
    request_count       BIGINT NOT NULL DEFAULT 0,
    error_count         BIGINT NOT NULL DEFAULT 0,
    tokens_in           BIGINT NOT NULL DEFAULT 0,
    tokens_out          BIGINT NOT NULL DEFAULT 0,
    tokens_cached       BIGINT NOT NULL DEFAULT 0,
    cost_micros         BIGINT NOT NULL DEFAULT 0,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (bucket, org_id, project_id, api_key_id, channel_key, model_actual, status_class)
);

CREATE INDEX IF NOT EXISTS usage_daily_rollups_org_bucket_idx ON usage_daily_rollups(org_id, bucket DESC);
CREATE INDEX IF NOT EXISTS usage_daily_rollups_bucket_idx ON usage_daily_rollups(bucket DESC);

CREATE TABLE IF NOT EXISTS billing_ledger_events (
    id                  UUID PRIMARY KEY,
    idempotency_key     TEXT NOT NULL UNIQUE,
    request_id          UUID,
    occurred_at         TIMESTAMPTZ NOT NULL,
    org_id              UUID NOT NULL,
    project_id          UUID NOT NULL,
    api_key_id          UUID NOT NULL,
    channel_id          UUID,
    direction           TEXT NOT NULL CHECK (direction IN ('debit', 'credit')),
    amount_micros       BIGINT NOT NULL CHECK (amount_micros >= 0),
    source_type         TEXT NOT NULL,
    source_id           TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'posted' CHECK (status IN ('pending', 'posted', 'voided')),
    metadata            JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS billing_ledger_events_request_idx ON billing_ledger_events(request_id);
CREATE INDEX IF NOT EXISTS billing_ledger_events_org_time_idx ON billing_ledger_events(org_id, occurred_at DESC);
