-- Revert to the original unique constraint on email only
-- Note: This will fail if there are duplicate emails across projects
DROP INDEX IF EXISTS users_email_project_unique;

-- Recreate the original unique index on email only
CREATE UNIQUE INDEX users_email_unique ON users (email) WHERE email IS NOT NULL;