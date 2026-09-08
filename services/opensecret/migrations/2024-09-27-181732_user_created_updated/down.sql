-- This file should undo anything in `up.sql`

-- Remove the trigger
DROP TRIGGER IF EXISTS trigger_update_users_updated_at ON users;

-- Remove the trigger function
DROP FUNCTION IF EXISTS update_users_updated_at();

-- Remove the columns
ALTER TABLE users
DROP COLUMN IF EXISTS created_at,
DROP COLUMN IF EXISTS updated_at;
