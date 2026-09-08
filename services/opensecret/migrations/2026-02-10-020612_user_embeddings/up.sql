-- ===========================================================
-- User Embeddings (Phase 1 RAG brute-force infrastructure)
-- ===========================================================

CREATE TABLE user_embeddings (
    id                   BIGSERIAL PRIMARY KEY,
    uuid                 UUID NOT NULL UNIQUE,
    user_id              UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,

    -- Source tracking
    -- v1 uses: 'message', 'archival'. Keep as open TEXT for future types.
    source_type          TEXT NOT NULL DEFAULT 'message',

    -- Message provenance (ONLY for source_type='message')
    user_message_id      BIGINT REFERENCES user_messages(id) ON DELETE CASCADE,
    assistant_message_id BIGINT REFERENCES assistant_messages(id) ON DELETE CASCADE,
    conversation_id      BIGINT REFERENCES conversations(id) ON DELETE CASCADE,

    -- Embedding vector
    vector_enc           BYTEA   NOT NULL, -- AES-256-GCM encrypted float32 array
    embedding_model      TEXT    NOT NULL, -- e.g. "nomic-embed-text"
    vector_dim           INTEGER NOT NULL DEFAULT 768,

    -- Content that was embedded
    content_enc          BYTEA NOT NULL,
    metadata_enc         BYTEA,

    -- Deterministically-encrypted tags (base64) for SQL-level filtering
    tags_enc             TEXT[] NOT NULL DEFAULT '{}',

    -- Plaintext metadata
    token_count          INTEGER NOT NULL,

    created_at           TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Enforce invariants for the v1 source types without blocking future types.
    CONSTRAINT user_embeddings_message_source_check CHECK (
        (source_type <> 'message')
        OR (
            -- exactly one of the message FKs must be set
            (user_message_id IS NOT NULL) <> (assistant_message_id IS NOT NULL)
        )
    ),
    CONSTRAINT user_embeddings_message_conversation_check CHECK (
        (source_type <> 'message') OR (conversation_id IS NOT NULL)
    ),
    CONSTRAINT user_embeddings_archival_source_check CHECK (
        (source_type <> 'archival')
        OR (
            user_message_id IS NULL
            AND assistant_message_id IS NULL
            AND conversation_id IS NULL
        )
    )
);

-- Primary query path: load all vectors for a user (brute-force scan)
CREATE INDEX idx_user_embeddings_user_id ON user_embeddings(user_id);

-- For time-filtered searches (recency bias)
CREATE INDEX idx_user_embeddings_user_created ON user_embeddings(user_id, created_at DESC);

-- For source-type filtered searches (message vs archival)
CREATE INDEX idx_user_embeddings_user_source ON user_embeddings(user_id, source_type);

-- For tag-filtered archival searches (encrypted, per-user tokens)
CREATE INDEX idx_user_embeddings_archival_tags_enc
    ON user_embeddings USING GIN(tags_enc)
    WHERE source_type = 'archival';

-- For conversation-scoped searches (message recall)
CREATE INDEX idx_user_embeddings_user_conversation ON user_embeddings(user_id, conversation_id);

-- Idempotency/deduplication: prevent double-indexing the same message
CREATE UNIQUE INDEX idx_user_embeddings_user_message_id
    ON user_embeddings(user_message_id)
    WHERE user_message_id IS NOT NULL;

CREATE UNIQUE INDEX idx_user_embeddings_assistant_message_id
    ON user_embeddings(assistant_message_id)
    WHERE assistant_message_id IS NOT NULL;

-- For staleness queries: find vectors that need re-embedding after model change
CREATE INDEX idx_user_embeddings_model ON user_embeddings(user_id, embedding_model);

-- Trigger for updated_at
-- Note: update_updated_at_column() function already exists from previous migrations
CREATE TRIGGER update_user_embeddings_updated_at
BEFORE UPDATE ON user_embeddings
FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
