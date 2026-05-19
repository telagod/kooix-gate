-- 配额原子预扣（pre-debit）
--
-- KEYS[1] = quota counter key, e.g. "quota:org:{org_id}:tokens:202605"
-- ARGV[1] = amount (i64)  要扣的额度（tokens 或请求数）
-- ARGV[2] = limit  (i64)  上限
-- ARGV[3] = ttl_seconds (i64) 计数器 TTL（一般是当月剩余秒数）
--
-- 返回：
--   { ok (0|1), current_used, remaining }
--
-- 算法：原子读 + 判 + 写。Lua 单线程保证不会发生 check-then-set race。
--   ok=0 时不增计数（调用方应 401/402 拒绝）。
--   ok=1 时调用方拿到 ok，业务做完再视情决定是否回滚（quota_refund.lua）。

local key   = KEYS[1]
local amt   = tonumber(ARGV[1])
local limit = tonumber(ARGV[2])
local ttl   = tonumber(ARGV[3])

local cur = tonumber(redis.call('GET', key) or '0')

if cur + amt > limit then
    return { 0, cur, math.max(0, limit - cur) }
end

local new_val = redis.call('INCRBY', key, amt)
-- 仅在第一次写入且 ttl > 0 时设 TTL（ttl <= 0 表示 lifetime counter，不过期）
if cur == 0 and ttl > 0 then
    redis.call('EXPIRE', key, ttl)
end

return { 1, new_val, math.max(0, limit - new_val) }
