-- Remove the trigger
DROP TRIGGER IF EXISTS trigger_update_email_verifications_updated_at ON email_verifications;

-- Remove the trigger function
DROP FUNCTION IF EXISTS update_email_verifications_updated_at();

-- Remove the indexes
DROP INDEX IF EXISTS idx_email_verifications_verification_code;
DROP INDEX IF EXISTS idx_email_verifications_user_id;

-- Drop the email_verifications table
DROP TABLE IF EXISTS email_verifications;
