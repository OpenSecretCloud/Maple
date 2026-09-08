-- Drop the existing unique index that enforces global email uniqueness
DROP INDEX IF EXISTS users_email_unique;

-- Create a composite unique index that allows the same email across different projects
-- but prevents duplicate emails within the same project
CREATE UNIQUE INDEX users_email_project_unique ON users (email, project_id) WHERE email IS NOT NULL;

-- The existing index idx_users_email_project_id already provides optimization for queries,
-- but this new unique index ensures data integrity at the database level