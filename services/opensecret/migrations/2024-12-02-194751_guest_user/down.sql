-- Remove the partial unique index
DROP INDEX IF EXISTS users_email_unique;

-- Remove guest users since email was required before this migration
DELETE FROM users WHERE email IS NULL;

-- Make email non-null again
ALTER TABLE users ALTER COLUMN email SET NOT NULL;

-- Restore the original unique constraint
ALTER TABLE users ADD CONSTRAINT users_email_key UNIQUE (email);
