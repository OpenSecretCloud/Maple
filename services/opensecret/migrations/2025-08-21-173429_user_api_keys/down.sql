-- Drop the user_api_keys table and its indexes
-- Drop trigger and function first
DROP TRIGGER IF EXISTS trigger_update_user_api_keys_updated_at ON user_api_keys;
DROP FUNCTION IF EXISTS update_user_api_keys_updated_at();

-- Drop index
DROP INDEX IF EXISTS idx_user_api_keys_user_id;

-- Drop table
DROP TABLE IF EXISTS user_api_keys;