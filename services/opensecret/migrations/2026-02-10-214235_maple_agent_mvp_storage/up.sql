-- Maple MVP agent storage tables
--
-- Note: update_updated_at_column() already exists from previous migrations.

CREATE TABLE memory_blocks (
    id          BIGSERIAL PRIMARY KEY,
    uuid        UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    user_id     UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,

    label       TEXT NOT NULL,
    value_enc   BYTEA NOT NULL,

    created_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    UNIQUE(user_id, label)
);

CREATE INDEX idx_memory_blocks_user_id ON memory_blocks(user_id);

CREATE TRIGGER update_memory_blocks_updated_at
BEFORE UPDATE ON memory_blocks
FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE conversation_summaries (
    id                  BIGSERIAL PRIMARY KEY,
    uuid                UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    user_id             UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    conversation_id     BIGINT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,

    from_created_at     TIMESTAMPTZ NOT NULL,
    to_created_at       TIMESTAMPTZ NOT NULL,
    message_count       INTEGER NOT NULL,

    content_enc         BYTEA NOT NULL,
    content_tokens      INTEGER NOT NULL,

    embedding_enc       BYTEA,

    previous_summary_id BIGINT REFERENCES conversation_summaries(id) ON DELETE SET NULL,

    created_at          TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT valid_time_range CHECK (from_created_at <= to_created_at)
);

CREATE INDEX idx_conversation_summaries_user_conv
    ON conversation_summaries(user_id, conversation_id, created_at DESC);

CREATE INDEX idx_conversation_summaries_chain
    ON conversation_summaries(previous_summary_id);

CREATE TABLE agents (
    id               BIGSERIAL PRIMARY KEY,
    uuid             UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    user_id          UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,

    conversation_id  BIGINT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    kind             TEXT NOT NULL,
    parent_agent_id  BIGINT REFERENCES agents(id) ON DELETE SET NULL,
    display_name_enc BYTEA,
    purpose_enc      BYTEA,
    created_by       TEXT NOT NULL DEFAULT 'user',

    created_at       TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT agents_kind_check CHECK (kind IN ('main', 'subagent')),
    CONSTRAINT agents_created_by_check CHECK (created_by IN ('user', 'agent')),
    CONSTRAINT agents_parent_check CHECK (
        (kind = 'main' AND parent_agent_id IS NULL)
        OR (kind = 'subagent' AND parent_agent_id IS NOT NULL)
    ),
    UNIQUE(conversation_id)
);

CREATE UNIQUE INDEX idx_agents_one_main_per_user
    ON agents(user_id)
    WHERE kind = 'main';

CREATE INDEX idx_agents_user_kind_created
    ON agents(user_id, kind, created_at DESC);

CREATE INDEX idx_agents_parent_agent_id
    ON agents(parent_agent_id);

CREATE TRIGGER update_agents_updated_at
BEFORE UPDATE ON agents
FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
