-- ============================================================================
-- usage_records.channel_id 改为可空
--
-- 原因：fallback provider 路径（没经 ProviderRouter 选 channel）的 UsageEvent
-- 没有 channel_id。之前用 uuid::nil() 占位会污染 channel 维度的统计与索引。
-- 改为允许 NULL，让分析查询用 WHERE channel_id IS NOT NULL 自然过滤。
-- ============================================================================

ALTER TABLE usage_records ALTER COLUMN channel_id DROP NOT NULL;
