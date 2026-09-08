CREATE TABLE password_reset_requests (
    id SERIAL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(uuid),
    hashed_secret VARCHAR(255) NOT NULL,
    encrypted_code BYTEA NOT NULL,
    expiration_time TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    is_reset BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_password_reset_requests_user_id ON password_reset_requests(user_id);
CREATE INDEX idx_password_reset_requests_encrypted_code ON password_reset_requests(encrypted_code);
