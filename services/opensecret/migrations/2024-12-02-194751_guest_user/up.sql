-- First drop the existing unique constraint and index on email
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_email_key;

-- Make email nullable 
ALTER TABLE users ALTER COLUMN email DROP NOT NULL;

-- Add a partial unique index that only applies to non-null emails
CREATE UNIQUE INDEX users_email_unique ON users (email) WHERE email IS NOT NULL;
