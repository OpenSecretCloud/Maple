CREATE TABLE platform_password_reset_requests (
    id SERIAL PRIMARY KEY,
    platform_user_id UUID NOT NULL REFERENCES platform_users(uuid) ON DELETE CASCADE,
    hashed_secret VARCHAR(255) NOT NULL,
    encrypted_code BYTEA NOT NULL,
    expiration_time TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    is_reset BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_platform_password_reset_requests_platform_user_id ON platform_password_reset_requests(platform_user_id);
CREATE INDEX idx_platform_password_reset_requests_encrypted_code ON platform_password_reset_requests(encrypted_code);

-- Add trigger for updated_at
CREATE TRIGGER update_platform_password_reset_requests_updated_at
    BEFORE UPDATE ON platform_password_reset_requests
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();
