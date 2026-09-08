ALTER TABLE user_messages
    ADD COLUMN assistant_reaction TEXT;

ALTER TABLE assistant_messages
    ADD COLUMN user_reaction TEXT;
