CREATE TABLE user_seed_wrappings (
    id BIGSERIAL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    credential_kind TEXT NOT NULL CHECK (credential_kind IN ('password', 'oauth')),
    credential_lookup_hash BYTEA NOT NULL,
    wrapping_version SMALLINT NOT NULL DEFAULT 1,
    seed_enc BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX idx_user_seed_wrappings_credential
    ON user_seed_wrappings(user_id, credential_kind, credential_lookup_hash, wrapping_version);

CREATE INDEX idx_user_seed_wrappings_user_kind
    ON user_seed_wrappings(user_id, credential_kind);

CREATE TRIGGER update_user_seed_wrappings_updated_at
BEFORE UPDATE ON user_seed_wrappings
FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE app_data_migrations (
    name TEXT PRIMARY KEY,
    completed_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
