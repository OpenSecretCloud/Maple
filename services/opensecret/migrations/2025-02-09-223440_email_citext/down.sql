-- Convert users email back to text
ALTER TABLE users ALTER COLUMN email TYPE text;

-- Drop the citext extension last (after all tables using it are dropped)
DROP EXTENSION IF EXISTS citext;
