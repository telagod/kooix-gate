-- 请求日志查询优化索引
-- keyset cursor 分页 + 错误过滤场景

-- 错误过滤：按状态 + 时间
CREATE INDEX CONCURRENTLY IF NOT EXISTS usage_records_org_status_ts_idx
    ON usage_records(org_id, status, ts DESC);

-- 按 request_id 直查单条（详情页）
CREATE INDEX CONCURRENTLY IF NOT EXISTS usage_records_request_id_idx
    ON usage_records(request_id);
