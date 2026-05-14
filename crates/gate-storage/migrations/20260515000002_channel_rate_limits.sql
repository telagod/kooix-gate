ALTER TABLE channels ADD COLUMN rpm_limit INT;
ALTER TABLE channels ADD COLUMN tpm_limit INT;
COMMENT ON COLUMN channels.rpm_limit IS 'Requests per minute limit (NULL = unlimited)';
COMMENT ON COLUMN channels.tpm_limit IS 'Tokens per minute limit (NULL = unlimited)';
