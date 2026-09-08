DROP TRIGGER IF EXISTS update_agents_updated_at ON agents;
DROP INDEX IF EXISTS idx_agents_parent_agent_id;
DROP INDEX IF EXISTS idx_agents_user_kind_created;
DROP INDEX IF EXISTS idx_agents_one_main_per_user;
DROP TABLE IF EXISTS agents;

DROP INDEX IF EXISTS idx_conversation_summaries_chain;
DROP INDEX IF EXISTS idx_conversation_summaries_user_conv;
DROP TABLE IF EXISTS conversation_summaries;

DROP TRIGGER IF EXISTS update_memory_blocks_updated_at ON memory_blocks;
DROP INDEX IF EXISTS idx_memory_blocks_user_id;
DROP TABLE IF EXISTS memory_blocks;
