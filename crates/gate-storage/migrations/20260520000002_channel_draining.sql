-- P1.4 Channel draining:
-- `draining` stops new routing because list_healthy_in_group only accepts
-- status='active', while existing in-flight requests can finish before the
-- channel is disabled.

ALTER TABLE channels
    DROP CONSTRAINT IF EXISTS channels_status_check;

ALTER TABLE channels
    ADD CONSTRAINT channels_status_check
    CHECK (status IN ('active', 'draining', 'disabled', 'deleted'));
