-- 扩展：UUID v7、citext（大小写不敏感字符串）、加密
CREATE EXTENSION IF NOT EXISTS "pgcrypto";
CREATE EXTENSION IF NOT EXISTS "citext";
CREATE EXTENSION IF NOT EXISTS "btree_gin";

-- TimescaleDB 用于 usage_records，按可用性启用
-- CREATE EXTENSION IF NOT EXISTS timescaledb;

-- updated_at 自动更新触发器
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
