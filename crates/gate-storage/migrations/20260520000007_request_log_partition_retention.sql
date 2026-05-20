-- Request log read model partitioning + retention helpers.
--
-- `request_events` remains the canonical idempotency / settlement table because
-- PostgreSQL unique constraints on partitioned tables must include the
-- partition key. Request-log list / filter / incident reads can use this
-- monthly-partitioned projection instead, while ledger remains the long-term
-- audit source of truth.

CREATE TABLE IF NOT EXISTS request_log_events (
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

    PRIMARY KEY (ts, request_id)
) PARTITION BY RANGE (ts);

CREATE INDEX IF NOT EXISTS request_log_events_request_id_idx ON request_log_events(request_id);
CREATE INDEX IF NOT EXISTS request_log_events_org_ts_idx ON request_log_events(org_id, ts DESC);
CREATE INDEX IF NOT EXISTS request_log_events_project_ts_idx ON request_log_events(project_id, ts DESC);
CREATE INDEX IF NOT EXISTS request_log_events_api_key_ts_idx ON request_log_events(api_key_id, ts DESC);
CREATE INDEX IF NOT EXISTS request_log_events_channel_ts_idx ON request_log_events(channel_id, ts DESC);
CREATE INDEX IF NOT EXISTS request_log_events_group_ts_idx ON request_log_events(group_id, ts DESC);
CREATE INDEX IF NOT EXISTS request_log_events_model_ts_idx ON request_log_events(model_actual, ts DESC);
CREATE INDEX IF NOT EXISTS request_log_events_status_ts_idx ON request_log_events(status, ts DESC);

CREATE OR REPLACE FUNCTION kooix_ensure_request_log_partition(for_ts TIMESTAMPTZ)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    part_start TIMESTAMPTZ := date_trunc('month', for_ts);
    part_end   TIMESTAMPTZ := part_start + INTERVAL '1 month';
    suffix     TEXT := to_char(part_start, 'YYYY_MM');
    part_name  TEXT := format('request_log_events_%s', suffix);
BEGIN
    IF to_regclass(part_name) IS NULL THEN
        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS %I PARTITION OF request_log_events FOR VALUES FROM (%L) TO (%L)',
            part_name,
            part_start,
            part_end
        );
    END IF;
    RETURN part_name;
END;
$$;

CREATE OR REPLACE FUNCTION kooix_ensure_request_log_partitions(months_ahead INT DEFAULT 3)
RETURNS TABLE(partition_name TEXT)
LANGUAGE plpgsql
AS $$
DECLARE
    base_month TIMESTAMPTZ := date_trunc('month', NOW());
    i          INT;
BEGIN
    months_ahead := GREATEST(0, LEAST(months_ahead, 24));
    FOR i IN 0..months_ahead LOOP
        partition_name := kooix_ensure_request_log_partition(base_month + (i || ' months')::INTERVAL);
        RETURN NEXT;
    END LOOP;
END;
$$;

CREATE OR REPLACE FUNCTION kooix_request_events_to_log_projection()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM kooix_ensure_request_log_partition(NEW.ts);
    INSERT INTO request_log_events (
        ts, request_id, idempotency_key,
        org_id, project_id, api_key_id, user_id,
        channel_id, channel_key_id, group_id,
        model_requested, model_actual, stream,
        tokens_in, tokens_out, tokens_cached, cost_micros, cost_usd,
        latency_ms, ttfb_ms, status, error_code, retries,
        created_at
    )
    VALUES (
        NEW.ts, NEW.request_id, NEW.idempotency_key,
        NEW.org_id, NEW.project_id, NEW.api_key_id, NEW.user_id,
        NEW.channel_id, NEW.channel_key_id, NEW.group_id,
        NEW.model_requested, NEW.model_actual, NEW.stream,
        NEW.tokens_in, NEW.tokens_out, NEW.tokens_cached, NEW.cost_micros, NEW.cost_usd,
        NEW.latency_ms, NEW.ttfb_ms, NEW.status, NEW.error_code, NEW.retries,
        NEW.created_at
    )
    ON CONFLICT (ts, request_id) DO NOTHING;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS request_events_log_projection_insert ON request_events;
CREATE TRIGGER request_events_log_projection_insert
AFTER INSERT ON request_events
FOR EACH ROW EXECUTE FUNCTION kooix_request_events_to_log_projection();

-- Pre-create the current month and the next three months so the first write of
-- a new deployment does not pay dynamic DDL cost.
SELECT kooix_ensure_request_log_partitions(3);

-- Backfill existing canonical rows into the partitioned request-log projection.
DO $$
DECLARE
    month_row RECORD;
BEGIN
    FOR month_row IN
        SELECT DISTINCT date_trunc('month', ts) AS month_start
        FROM request_events
    LOOP
        PERFORM kooix_ensure_request_log_partition(month_row.month_start);
    END LOOP;
END $$;

INSERT INTO request_log_events (
    ts, request_id, idempotency_key,
    org_id, project_id, api_key_id, user_id,
    channel_id, channel_key_id, group_id,
    model_requested, model_actual, stream,
    tokens_in, tokens_out, tokens_cached, cost_micros, cost_usd,
    latency_ms, ttfb_ms, status, error_code, retries,
    created_at
)
SELECT
    ts, request_id, idempotency_key,
    org_id, project_id, api_key_id, user_id,
    channel_id, channel_key_id, group_id,
    model_requested, model_actual, stream,
    tokens_in, tokens_out, tokens_cached, cost_micros, cost_usd,
    latency_ms, ttfb_ms, status, error_code, retries,
    created_at
FROM request_events
ON CONFLICT (ts, request_id) DO NOTHING;

CREATE OR REPLACE FUNCTION kooix_prune_request_log_partitions(
    retention_months INT DEFAULT 18,
    dry_run BOOLEAN DEFAULT TRUE
)
RETURNS TABLE(partition_name TEXT, partition_month TEXT, dropped BOOLEAN)
LANGUAGE plpgsql
AS $$
DECLARE
    cutoff_suffix TEXT;
    part RECORD;
BEGIN
    retention_months := GREATEST(1, LEAST(retention_months, 120));
    cutoff_suffix := to_char(
        date_trunc('month', NOW()) - (retention_months || ' months')::INTERVAL,
        'YYYY_MM'
    );

    FOR part IN
        SELECT child.relname AS relname,
               substring(child.relname FROM '([0-9]{4}_[0-9]{2})$') AS suffix
        FROM pg_inherits i
        JOIN pg_class parent ON parent.oid = i.inhparent
        JOIN pg_class child ON child.oid = i.inhrelid
        JOIN pg_namespace ns ON ns.oid = child.relnamespace
        WHERE parent.oid = 'request_log_events'::regclass
          AND ns.nspname = current_schema()
          AND child.relname ~ '^request_log_events_[0-9]{4}_[0-9]{2}$'
        ORDER BY child.relname
    LOOP
        IF part.suffix IS NOT NULL AND part.suffix < cutoff_suffix THEN
            partition_name := part.relname;
            partition_month := part.suffix;
            dropped := FALSE;
            IF NOT dry_run THEN
                EXECUTE format('DROP TABLE IF EXISTS %I', part.relname);
                dropped := TRUE;
            END IF;
            RETURN NEXT;
        END IF;
    END LOOP;
END;
$$;

CREATE OR REPLACE FUNCTION kooix_prune_request_log_details(retention_days INT DEFAULT 540)
RETURNS BIGINT
LANGUAGE plpgsql
AS $$
DECLARE
    deleted_rows BIGINT;
BEGIN
    retention_days := GREATEST(1, LEAST(retention_days, 3650));
    DELETE FROM request_event_details
    WHERE ts < NOW() - (retention_days || ' days')::INTERVAL;
    GET DIAGNOSTICS deleted_rows = ROW_COUNT;
    RETURN deleted_rows;
END;
$$;
