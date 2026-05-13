-- 滑动窗口限流（sliding window log，Redis 原子）
--
-- KEYS[1] = window key, e.g. "rl:apikey:{api_key_id}:1m"
-- ARGV[1] = now_ms (i64)
-- ARGV[2] = window_ms (i64)  e.g. 60000 for 1 minute
-- ARGV[3] = limit (i64)
-- ARGV[4] = req_id (string, unique per request) — 用作 ZSET member 防去重消失
--
-- 返回：
--   { allowed (0|1), current_count, remaining, retry_after_ms }
--
-- 算法：
--   1. 移除窗口外的旧 entry (score <= now - window_ms)
--   2. ZCARD 当前条数
--   3. 若 < limit：ZADD now/req_id；返回 allowed=1
--   4. 否则：取最早 entry 算 retry_after_ms；返回 allowed=0
--
-- 注意：req_id 必须唯一，否则同一秒 N 个请求被去重就漏统计了。
--       由调用方传 ULID/UUID 即可。

local key       = KEYS[1]
local now       = tonumber(ARGV[1])
local window    = tonumber(ARGV[2])
local limit     = tonumber(ARGV[3])
local req_id    = ARGV[4]

local cutoff = now - window

-- 1. 移除窗口外
redis.call('ZREMRANGEBYSCORE', key, '-inf', cutoff)

-- 2. 当前数
local count = tonumber(redis.call('ZCARD', key))

if count < limit then
    -- 3. 允许：写入新 entry
    redis.call('ZADD', key, now, req_id)
    redis.call('PEXPIRE', key, window)
    return { 1, count + 1, limit - count - 1, 0 }
else
    -- 4. 拒绝：算 retry_after_ms = 最早 entry 出窗时刻 - now
    local oldest = redis.call('ZRANGE', key, 0, 0, 'WITHSCORES')
    local retry = 0
    if oldest[2] then
        retry = (tonumber(oldest[2]) + window) - now
        if retry < 0 then retry = 0 end
    end
    return { 0, count, 0, retry }
end
