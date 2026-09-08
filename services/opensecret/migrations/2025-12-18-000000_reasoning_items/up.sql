-- ===========================================================
-- Reasoning Items Table Migration
-- ===========================================================
-- Stores reasoning/chain-of-thought content from thinking models
-- (for models that emit reasoning tokens)
-- 
-- Per OpenAI's Responses API spec, reasoning is a separate 
-- conversation item type, similar to messages and tool_calls.
-- ===========================================================

-- reasoning_items table - Reasoning chain-of-thought from thinking models
-- response_id: NULL if created via Conversations API, populated if created via Responses API
-- assistant_message_id: Links reasoning to specific assistant message (1:1 relationship)
--   This allows multiple assistant messages per response to each have their own reasoning
-- content_enc: NULL while streaming, populated when completed
-- status: in_progress (streaming), completed (done), incomplete (partial)
CREATE TABLE reasoning_items (
    id                    BIGSERIAL PRIMARY KEY,
    uuid                  UUID    NOT NULL UNIQUE,
    conversation_id       BIGINT  NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    response_id           BIGINT  REFERENCES responses(id) ON DELETE CASCADE,
    assistant_message_id  BIGINT  REFERENCES assistant_messages(id) ON DELETE CASCADE,
    user_id               UUID    NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    content_enc           BYTEA,
    summary_enc           BYTEA,
    reasoning_tokens      INTEGER NOT NULL DEFAULT 0,
    status                TEXT    NOT NULL DEFAULT 'in_progress' CHECK (status IN ('in_progress','completed','incomplete')),
    created_at            TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Indexes for reasoning_items
CREATE INDEX idx_reasoning_items_conversation_id ON reasoning_items(conversation_id);
CREATE INDEX idx_reasoning_items_response_id ON reasoning_items(response_id);
CREATE INDEX idx_reasoning_items_assistant_message_id ON reasoning_items(assistant_message_id);
CREATE INDEX idx_reasoning_items_conversation_created_id
    ON reasoning_items(conversation_id, created_at DESC, id);

-- Trigger for updated_at
CREATE TRIGGER update_reasoning_items_updated_at
BEFORE UPDATE ON reasoning_items
FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
