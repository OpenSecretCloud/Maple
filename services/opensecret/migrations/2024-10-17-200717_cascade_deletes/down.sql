-- Revert cascading deletes from all tables referencing users

-- Revert the foreign key constraint change on user_kv table
ALTER TABLE user_kv
DROP CONSTRAINT user_kv_user_id_fkey,
ADD CONSTRAINT user_kv_user_id_fkey
    FOREIGN KEY (user_id)
    REFERENCES users(uuid);

-- Revert the foreign key constraint change on email_verifications table
ALTER TABLE email_verifications
DROP CONSTRAINT email_verifications_user_id_fkey,
ADD CONSTRAINT email_verifications_user_id_fkey
    FOREIGN KEY (user_id)
    REFERENCES users(uuid);

-- Revert the foreign key constraint change on password_reset_requests table
ALTER TABLE password_reset_requests
DROP CONSTRAINT password_reset_requests_user_id_fkey,
ADD CONSTRAINT password_reset_requests_user_id_fkey
    FOREIGN KEY (user_id)
    REFERENCES users(uuid);
