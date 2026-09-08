-- Create platform_users table (developers/admins who build apps)
CREATE TABLE platform_users (
    id SERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT uuid_generate_v4() UNIQUE,
    email CITEXT NOT NULL UNIQUE,
    name TEXT,
    password_enc BYTEA,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create organizations table
CREATE TABLE orgs (
    id SERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT uuid_generate_v4() UNIQUE,
    name TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create organization memberships table (many-to-many relationship between platform_users and orgs)
-- A platform user can only have one role per organization
CREATE TABLE org_memberships (
    id SERIAL PRIMARY KEY,
    platform_user_id UUID NOT NULL REFERENCES platform_users(uuid) ON DELETE CASCADE,
    org_id INTEGER NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'developer', 'viewer')),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(platform_user_id, org_id)  
);

-- Create organization projects table
CREATE TABLE org_projects (
    id SERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT uuid_generate_v4() UNIQUE,
    client_id UUID NOT NULL DEFAULT uuid_generate_v4() UNIQUE,
    org_id INTEGER NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive', 'suspended')),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(org_id, name)
);

-- Create project secrets table for storing encrypted API keys, OAuth secrets, etc.
-- Each key name should be unique per project
CREATE TABLE org_project_secrets (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES org_projects(id) ON DELETE CASCADE,
    key_name TEXT NOT NULL,
    secret_enc BYTEA NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(project_id, key_name)
);

-- Create project_settings table for storing project-specific configurations
CREATE TABLE project_settings (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES org_projects(id) ON DELETE CASCADE,
    category TEXT NOT NULL,
    settings JSONB NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(project_id, category)
);

-- Create invite codes table for organization invitations
CREATE TABLE invite_codes (
    id SERIAL PRIMARY KEY,
    code UUID NOT NULL DEFAULT uuid_generate_v4() UNIQUE,
    org_id INTEGER NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
    email TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'developer', 'viewer')),
    used BOOLEAN NOT NULL DEFAULT false,
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Add indexes for foreign keys and commonly queried fields
CREATE INDEX idx_platform_users_email ON platform_users(email);
CREATE INDEX idx_org_memberships_platform_user_id ON org_memberships(platform_user_id);
CREATE INDEX idx_org_memberships_org_id ON org_memberships(org_id);
CREATE INDEX idx_org_projects_org_id ON org_projects(org_id);
CREATE INDEX idx_org_projects_client_id ON org_projects(client_id);
CREATE INDEX idx_org_project_secrets_project_id ON org_project_secrets(project_id);
CREATE INDEX idx_invite_codes_org_id ON invite_codes(org_id);
CREATE INDEX idx_invite_codes_code ON invite_codes(code);
CREATE INDEX idx_invite_codes_email ON invite_codes(email);

-- Add index for project_settings
CREATE INDEX idx_project_settings_project_id ON project_settings(project_id);

-- Create updated_at triggers
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_platform_users_updated_at
    BEFORE UPDATE ON platform_users
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_orgs_updated_at
    BEFORE UPDATE ON orgs
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_org_memberships_updated_at
    BEFORE UPDATE ON org_memberships
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_org_projects_updated_at
    BEFORE UPDATE ON org_projects
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_org_project_secrets_updated_at
    BEFORE UPDATE ON org_project_secrets
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_invite_codes_updated_at
    BEFORE UPDATE ON invite_codes
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_project_settings_updated_at
    BEFORE UPDATE ON project_settings
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

-- Create a default "OpenSecret" organization and "Maple" project for existing data
INSERT INTO orgs (name) VALUES ('OpenSecret');

-- Create the Maple project under OpenSecret organization
INSERT INTO org_projects (
    org_id,
    name,
    description,
    status,
    client_id
)
SELECT
    id,
    'Maple',
    'TryMaple Project',
    'active',
    'ba5a14b5-d915-47b1-b7b1-afda52bc5fc6'::uuid
FROM orgs
WHERE name = 'OpenSecret';

-- Add project_id to users table and set all existing users to Maple project
ALTER TABLE users ADD COLUMN project_id INTEGER REFERENCES org_projects(id) ON DELETE CASCADE;
CREATE INDEX idx_users_project_id ON users(project_id);

-- Set all existing users to belong to the Maple project
UPDATE users SET project_id = (
    SELECT op.id
    FROM org_projects op
    JOIN orgs o ON op.org_id = o.id
    WHERE o.name = 'OpenSecret' AND op.name = 'Maple'
);

-- Verify all users have been migrated successfully
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM users WHERE project_id IS NULL) THEN
        RAISE EXCEPTION 'Migration failed: Some users have not been assigned to a project';
    END IF;
END $$;

-- Make project_id required
ALTER TABLE users ALTER COLUMN project_id SET NOT NULL;

-- Add composite index for users table to optimize email + project_id queries
CREATE INDEX idx_users_email_project_id ON users(email, project_id);
