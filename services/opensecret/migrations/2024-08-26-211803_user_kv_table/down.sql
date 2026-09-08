DROP TRIGGER IF EXISTS trigger_update_user_kv_updated_at ON user_kv;
DROP FUNCTION IF EXISTS update_user_kv_updated_at();
DROP INDEX IF EXISTS idx_user_kv_user_id;
DROP INDEX IF EXISTS idx_user_kv_user_id_key_enc;
DROP TABLE IF EXISTS user_kv;
