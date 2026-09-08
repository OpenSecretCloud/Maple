-- ===========================================================
-- Conversation Projects and Pinned Chats Migration
-- ===========================================================

CREATE TABLE conversation_projects (
    id         BIGSERIAL PRIMARY KEY,
    uuid       UUID        NOT NULL UNIQUE,
    user_id    UUID        NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    name_enc   BYTEA       NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_conversation_projects_updated_id
    ON conversation_projects(user_id, updated_at DESC, id DESC);

CREATE TRIGGER update_conversation_projects_updated_at
BEFORE UPDATE ON conversation_projects
FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Projects intentionally own their conversations, so deleting a project
-- should delete the conversations assigned to it.
ALTER TABLE conversations
    ADD COLUMN project_id BIGINT REFERENCES conversation_projects(id) ON DELETE CASCADE,
    ADD COLUMN is_pinned BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN last_activity_at TIMESTAMPTZ;

-- Support FK cascade lookups, which filter by project_id without user_id.
CREATE INDEX idx_conversations_project_id
    ON conversations(project_id)
    WHERE project_id IS NOT NULL;

CREATE INDEX idx_conversations_last_activity_id
    ON conversations(user_id, last_activity_at DESC, id DESC);

CREATE INDEX idx_conversations_project_last_activity_id
    ON conversations(user_id, project_id, last_activity_at DESC, id DESC);

CREATE INDEX idx_conversations_pinned_last_activity_id
    ON conversations(user_id, is_pinned, last_activity_at DESC, id DESC);

DROP INDEX IF EXISTS idx_conversations_updated_id;

ALTER TABLE user_instructions
    ADD COLUMN project_id BIGINT REFERENCES conversation_projects(id) ON DELETE CASCADE;

ALTER TABLE user_instructions
    ALTER COLUMN name_enc DROP NOT NULL;

CREATE UNIQUE INDEX idx_user_instructions_project_id_unique
    ON user_instructions(project_id)
    WHERE project_id IS NOT NULL;

ALTER TABLE user_instructions
    ADD CONSTRAINT chk_user_instructions_project_not_default
    CHECK (project_id IS NULL OR is_default = FALSE),
    ADD CONSTRAINT chk_user_instructions_global_name_required
    CHECK (project_id IS NOT NULL OR name_enc IS NOT NULL);

-- Conversation recency is intentionally based on user-visible chat activity only.
-- Tool calls, tool outputs, and reasoning items are excluded so internal agent work
-- does not reorder history/sidebar views or add unnecessary parent-row churn.
WITH latest_activity AS (
    SELECT
        conversation_id,
        MAX(activity_at) AS latest_activity_at
    FROM (
        SELECT conversation_id, COALESCE(updated_at, created_at) AS activity_at FROM user_messages
        UNION ALL
        SELECT conversation_id, COALESCE(updated_at, created_at) AS activity_at FROM assistant_messages
    ) activity
    GROUP BY conversation_id
)
UPDATE conversations
SET last_activity_at = latest_activity.latest_activity_at
FROM latest_activity
WHERE conversations.id = latest_activity.conversation_id;

UPDATE conversations
SET last_activity_at = created_at
WHERE last_activity_at IS NULL;

ALTER TABLE conversations
    ALTER COLUMN last_activity_at SET DEFAULT CURRENT_TIMESTAMP,
    ALTER COLUMN last_activity_at SET NOT NULL;

-- Keep recency maintenance in the database so every write path stays consistent
-- without requiring separate application-managed parent updates.
CREATE OR REPLACE FUNCTION touch_conversation_last_activity_at_from_child()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE conversations
    SET last_activity_at = CURRENT_TIMESTAMP
    WHERE id = COALESCE(NEW.conversation_id, OLD.conversation_id);

    RETURN COALESCE(NEW, OLD);
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER touch_conversation_last_activity_at_from_user_messages
AFTER INSERT OR UPDATE ON user_messages
FOR EACH ROW EXECUTE FUNCTION touch_conversation_last_activity_at_from_child();

CREATE TRIGGER touch_conversation_last_activity_at_from_assistant_messages
AFTER INSERT OR UPDATE ON assistant_messages
FOR EACH ROW EXECUTE FUNCTION touch_conversation_last_activity_at_from_child();
