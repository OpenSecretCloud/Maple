-- Reverse reasoning_items migration

DROP TRIGGER IF EXISTS update_reasoning_items_updated_at ON reasoning_items;
DROP INDEX IF EXISTS idx_reasoning_items_conversation_created_id;
DROP INDEX IF EXISTS idx_reasoning_items_assistant_message_id;
DROP INDEX IF EXISTS idx_reasoning_items_response_id;
DROP INDEX IF EXISTS idx_reasoning_items_conversation_id;
DROP TABLE IF EXISTS reasoning_items;
