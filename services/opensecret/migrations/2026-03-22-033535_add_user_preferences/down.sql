DROP TRIGGER IF EXISTS update_user_preferences_updated_at ON user_preferences;
DROP INDEX IF EXISTS idx_user_preferences_user_id;
DROP TABLE IF EXISTS user_preferences;
