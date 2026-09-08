DROP TABLE IF EXISTS app_data_migrations;

DROP TRIGGER IF EXISTS update_user_seed_wrappings_updated_at ON user_seed_wrappings;
DROP TABLE IF EXISTS user_seed_wrappings;
