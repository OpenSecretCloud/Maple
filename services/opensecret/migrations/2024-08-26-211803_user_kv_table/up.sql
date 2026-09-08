CREATE TABLE user_kv (
    id BIGSERIAL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(uuid),
    key_enc BYTEA NOT NULL,
    value_enc BYTEA NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (user_id, key_enc)
);

-- Create an index on user_id for faster lookups
CREATE INDEX idx_user_kv_user_id ON user_kv(user_id);
CREATE INDEX idx_user_kv_user_id_key_enc ON user_kv(user_id, key_enc);

-- Create a trigger to automatically update the updated_at column
CREATE OR REPLACE FUNCTION update_user_kv_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_update_user_kv_updated_at
BEFORE UPDATE ON user_kv
FOR EACH ROW
EXECUTE FUNCTION update_user_kv_updated_at();
