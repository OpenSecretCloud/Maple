-- Create table for storing user API keys (hash only, never the actual key)
-- key_hash stores SHA-256 hash of UUID with dashes
CREATE TABLE user_api_keys (
    id SERIAL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    key_hash VARCHAR(64) NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, name)
);

-- Index for efficient user lookups (key_hash already has unique index)
CREATE INDEX idx_user_api_keys_user_id ON user_api_keys(user_id);

-- Create a trigger to automatically update the updated_at column
CREATE OR REPLACE FUNCTION update_user_api_keys_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_update_user_api_keys_updated_at
BEFORE UPDATE ON user_api_keys
FOR EACH ROW
EXECUTE FUNCTION update_user_api_keys_updated_at();