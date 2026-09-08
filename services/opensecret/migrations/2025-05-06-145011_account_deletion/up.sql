-- Create account deletion requests table without direct FK references
-- to preserve deletion records after user is deleted
CREATE TABLE account_deletion_requests (
    id SERIAL PRIMARY KEY,
    user_id UUID NOT NULL,
    project_id INTEGER NOT NULL,
    hashed_secret VARCHAR(255) NOT NULL,
    encrypted_code BYTEA NOT NULL,
    expiration_time TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    completed_at TIMESTAMP WITH TIME ZONE,
    is_deleted BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_account_deletion_requests_user_id ON account_deletion_requests(user_id);
CREATE INDEX idx_account_deletion_requests_encrypted_code ON account_deletion_requests(encrypted_code);
