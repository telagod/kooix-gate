-- 配额回滚（quota refund）
--
-- 用于：预扣后业务失败、或流式 3-stage billing 的"实际消耗 < 预扣"差额回退。
--
-- KEYS[1] = quota counter key
-- ARGV[1] = amount (i64)  正数，回滚多少
--
-- 返回 current_used（回滚后）

local key = KEYS[1]
local amt = tonumber(ARGV[1])

local cur = tonumber(redis.call('GET', key) or '0')
local new_val = cur - amt
if new_val < 0 then new_val = 0 end
redis.call('SET', key, tostring(new_val), 'KEEPTTL')
return new_val
