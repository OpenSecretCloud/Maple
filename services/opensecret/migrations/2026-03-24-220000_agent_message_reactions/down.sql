ALTER TABLE assistant_messages
    DROP COLUMN IF EXISTS user_reaction;

ALTER TABLE user_messages
    DROP COLUMN IF EXISTS assistant_reaction;
