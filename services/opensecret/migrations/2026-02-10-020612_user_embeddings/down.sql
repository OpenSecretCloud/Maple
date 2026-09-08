DROP TRIGGER IF EXISTS update_user_embeddings_updated_at ON user_embeddings;

DROP INDEX IF EXISTS idx_user_embeddings_user_id;
DROP INDEX IF EXISTS idx_user_embeddings_user_created;
DROP INDEX IF EXISTS idx_user_embeddings_user_source;
DROP INDEX IF EXISTS idx_user_embeddings_archival_tags_enc;
DROP INDEX IF EXISTS idx_user_embeddings_user_conversation;
DROP INDEX IF EXISTS idx_user_embeddings_user_message_id;
DROP INDEX IF EXISTS idx_user_embeddings_assistant_message_id;
DROP INDEX IF EXISTS idx_user_embeddings_model;

DROP TABLE IF EXISTS user_embeddings;
