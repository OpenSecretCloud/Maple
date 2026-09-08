-- Drop triggers
DROP TRIGGER IF EXISTS trigger_update_user_oauth_connections_updated_at ON user_oauth_connections;
DROP TRIGGER IF EXISTS trigger_update_oauth_providers_updated_at ON oauth_providers;

-- Drop functions
DROP FUNCTION IF EXISTS update_user_oauth_connections_updated_at();
DROP FUNCTION IF EXISTS update_oauth_providers_updated_at();

-- Drop indexes
DROP INDEX IF EXISTS idx_user_oauth_connections_provider_id;
DROP INDEX IF EXISTS idx_user_oauth_connections_user_id;

-- Drop tables
DROP TABLE IF EXISTS user_oauth_connections;
DROP TABLE IF EXISTS oauth_providers;

-- Remove users without passwords since this was a requirement before this migration
DELETE FROM users WHERE password_enc IS NULL;

-- Revert users table change
ALTER TABLE users ALTER COLUMN password_enc SET NOT NULL;
