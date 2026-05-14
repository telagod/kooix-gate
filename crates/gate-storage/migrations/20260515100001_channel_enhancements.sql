-- P1: Channel management enhancements
-- Adds tags, model_mapping, and indexes for filtering/sorting

ALTER TABLE channels ADD COLUMN IF NOT EXISTS tags TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE channels ADD COLUMN IF NOT EXISTS model_mapping JSONB NOT NULL DEFAULT '{}';

-- Index for provider_type filter
CREATE INDEX IF NOT EXISTS idx_channels_provider_type ON channels (provider_type) WHERE deleted_at IS NULL;

-- Index for status filter
CREATE INDEX IF NOT EXISTS idx_channels_status ON channels (status) WHERE deleted_at IS NULL;

-- Index for health filter
CREATE INDEX IF NOT EXISTS idx_channels_health ON channels (health) WHERE deleted_at IS NULL;

-- GIN index for tags array search
CREATE INDEX IF NOT EXISTS idx_channels_tags ON channels USING GIN (tags) WHERE deleted_at IS NULL;
