-- Create oauth_providers table
CREATE TABLE oauth_providers (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    auth_url TEXT NOT NULL,
    token_url TEXT NOT NULL,
    user_info_url TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create user_oauth_connections table
CREATE TABLE user_oauth_connections (
    id SERIAL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    provider_id INTEGER NOT NULL REFERENCES oauth_providers(id),
    provider_user_id VARCHAR(255) NOT NULL,
    access_token_enc BYTEA NOT NULL,
    refresh_token_enc BYTEA,
    expires_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (user_id, provider_id, provider_user_id)
);

-- Alter users table to allow NULL passwords for OAuth-only users
ALTER TABLE users ALTER COLUMN password_enc DROP NOT NULL;

-- Create indexes
CREATE INDEX idx_user_oauth_connections_user_id ON user_oauth_connections(user_id);
CREATE INDEX idx_user_oauth_connections_provider_id ON user_oauth_connections(provider_id);

-- Create triggers for updating timestamps
CREATE OR REPLACE FUNCTION update_oauth_providers_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_update_oauth_providers_updated_at
BEFORE UPDATE ON oauth_providers
FOR EACH ROW EXECUTE FUNCTION update_oauth_providers_updated_at();

CREATE OR REPLACE FUNCTION update_user_oauth_connections_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_update_user_oauth_connections_updated_at
BEFORE UPDATE ON user_oauth_connections
FOR EACH ROW EXECUTE FUNCTION update_user_oauth_connections_updated_at();
