-- ============================================================================
-- Usage Records: 时序数据，单表月级 20 亿行预算
-- 用 TimescaleDB 最优，未启用时退化为月分区表
-- ============================================================================

CREATE TABLE usage_records (
    ts              TIMESTAMPTZ NOT NULL,
    request_id      UUID NOT NULL,

    -- 归属
    org_id          UUID NOT NULL,
    project_id      UUID NOT NULL,
    api_key_id      UUID NOT NULL,
    user_id         UUID,                                -- 调用 key 时不一定有

    -- 路由
    channel_id      UUID NOT NULL,
    channel_key_id  UUID,
    group_id        UUID,

    -- 业务
    model_requested TEXT NOT NULL,                       -- 用户传的 alias
    model_actual    TEXT NOT NULL,                       -- 真实落到上游的模型
    stream          BOOLEAN NOT NULL DEFAULT FALSE,

    tokens_in       INT NOT NULL DEFAULT 0,
    tokens_out      INT NOT NULL DEFAULT 0,
    tokens_cached   INT NOT NULL DEFAULT 0,
    cost_usd        NUMERIC(12, 8) NOT NULL DEFAULT 0,

    -- 表现
    latency_ms      INT,
    ttfb_ms         INT,                                 -- time to first byte (流式)
    status          SMALLINT NOT NULL,                   -- HTTP status code
    error_code      TEXT,
    retries         SMALLINT NOT NULL DEFAULT 0,

    client_ip       INET,
    metadata        JSONB,

    PRIMARY KEY (ts, request_id)
);

-- 关键索引（针对常见查询模式）
CREATE INDEX usage_records_project_ts_idx ON usage_records(project_id, ts DESC);
CREATE INDEX usage_records_org_ts_idx ON usage_records(org_id, ts DESC);
CREATE INDEX usage_records_api_key_ts_idx ON usage_records(api_key_id, ts DESC);
CREATE INDEX usage_records_channel_ts_idx ON usage_records(channel_id, ts DESC);
CREATE INDEX usage_records_model_ts_idx ON usage_records(model_actual, ts DESC);

-- TimescaleDB hypertable（若扩展已启用，注释解除）
-- SELECT create_hypertable('usage_records', 'ts', chunk_time_interval => INTERVAL '1 day');
-- ALTER TABLE usage_records SET (
--     timescaledb.compress,
--     timescaledb.compress_segmentby = 'project_id, channel_id',
--     timescaledb.compress_orderby = 'ts DESC'
-- );
-- SELECT add_compression_policy('usage_records', INTERVAL '7 days');
-- SELECT add_retention_policy('usage_records', INTERVAL '90 days');

-- ============================================================================
-- 流式请求 in-flight 表（预扣 + 修正用）
-- ============================================================================
CREATE TABLE inflight_requests (
    request_id      UUID PRIMARY KEY,
    project_id      UUID NOT NULL,
    api_key_id      UUID NOT NULL,
    channel_id      UUID NOT NULL,
    channel_key_id  UUID,
    model           TEXT NOT NULL,
    estimated_cost_usd NUMERIC(12, 8) NOT NULL,
    estimated_tokens INT NOT NULL,
    started_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at      TIMESTAMPTZ NOT NULL                  -- 超时后由清扫任务回滚
);

CREATE INDEX inflight_requests_expires_idx ON inflight_requests(expires_at);
CREATE INDEX inflight_requests_project_idx ON inflight_requests(project_id);
