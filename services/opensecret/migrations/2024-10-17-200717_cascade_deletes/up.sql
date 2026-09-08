-- Add cascading deletes to all tables referencing users

-- Modify the foreign key constraint on user_kv table
ALTER TABLE user_kv
DROP CONSTRAINT user_kv_user_id_fkey,
ADD CONSTRAINT user_kv_user_id_fkey
    FOREIGN KEY (user_id)
    REFERENCES users(uuid)
    ON DELETE CASCADE;

-- Modify the foreign key constraint on email_verifications table
ALTER TABLE email_verifications
DROP CONSTRAINT email_verifications_user_id_fkey,
ADD CONSTRAINT email_verifications_user_id_fkey
    FOREIGN KEY (user_id)
    REFERENCES users(uuid)
    ON DELETE CASCADE;

-- Modify the foreign key constraint on password_reset_requests table
ALTER TABLE password_reset_requests
DROP CONSTRAINT password_reset_requests_user_id_fkey,
ADD CONSTRAINT password_reset_requests_user_id_fkey
    FOREIGN KEY (user_id)
    REFERENCES users(uuid)
    ON DELETE CASCADE;
