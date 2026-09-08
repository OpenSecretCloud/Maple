-- Drop the trigger first
DROP TRIGGER IF EXISTS trigger_update_platform_email_verifications_updated_at ON platform_email_verifications;

-- Drop indexes
DROP INDEX IF EXISTS idx_platform_email_verifications_verification_code;
DROP INDEX IF EXISTS idx_platform_email_verifications_platform_user_id;

-- Drop the table last
DROP TABLE IF EXISTS platform_email_verifications;
