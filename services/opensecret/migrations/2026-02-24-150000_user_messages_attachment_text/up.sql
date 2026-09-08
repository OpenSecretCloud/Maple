-- Add optional derived image description storage for user messages
--
-- This stores Maple-style vision pre-processing results (encrypted in app layer)
-- without mutating the original MessageContent JSON.

ALTER TABLE user_messages
    ADD COLUMN attachment_text_enc BYTEA;
