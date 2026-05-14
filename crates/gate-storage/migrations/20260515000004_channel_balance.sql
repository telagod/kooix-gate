ALTER TABLE channels ADD COLUMN balance DOUBLE PRECISION;
ALTER TABLE channels ADD COLUMN balance_updated_at TIMESTAMPTZ;
COMMENT ON COLUMN channels.balance IS 'Upstream account balance in USD (NULL = unknown/unsupported)';
