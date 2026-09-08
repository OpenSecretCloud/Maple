-- Enable citext extension for case-insensitive text fields
CREATE EXTENSION IF NOT EXISTS citext;

-- Modify existing users table to use citext for email
ALTER TABLE users ALTER COLUMN email TYPE citext;
