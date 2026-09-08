-- Drop triggers first
DROP TRIGGER IF EXISTS update_invite_codes_updated_at ON invite_codes;
DROP TRIGGER IF EXISTS update_org_project_secrets_updated_at ON org_project_secrets;
DROP TRIGGER IF EXISTS update_org_projects_updated_at ON org_projects;
DROP TRIGGER IF EXISTS update_org_memberships_updated_at ON org_memberships;
DROP TRIGGER IF EXISTS update_orgs_updated_at ON orgs;
DROP TRIGGER IF EXISTS update_platform_users_updated_at ON platform_users;
DROP TRIGGER IF EXISTS update_project_settings_updated_at ON project_settings;

-- Drop project_settings table
DROP TABLE IF EXISTS project_settings;

-- Drop project_id from users table
DROP INDEX IF EXISTS idx_users_project_id;
ALTER TABLE users DROP COLUMN IF EXISTS project_id;

-- Drop tables in reverse order (to handle foreign key dependencies)
DROP TABLE IF EXISTS invite_codes;
DROP TABLE IF EXISTS org_project_secrets;
DROP TABLE IF EXISTS org_projects;
DROP TABLE IF EXISTS org_memberships;
DROP TABLE IF EXISTS orgs;
DROP TABLE IF EXISTS platform_users;

-- Drop the trigger function (moved to end after all triggers are dropped)
DROP FUNCTION IF EXISTS update_updated_at_column();
