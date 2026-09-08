CREATE TABLE platform_email_verifications (
    id SERIAL PRIMARY KEY,
    platform_user_id UUID NOT NULL REFERENCES platform_users(uuid),
    verification_code UUID NOT NULL DEFAULT uuid_generate_v4(),
    is_verified BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL
);

-- Create indexes for faster lookups
CREATE INDEX idx_platform_email_verifications_platform_user_id ON platform_email_verifications(platform_user_id);
CREATE INDEX idx_platform_email_verifications_verification_code ON platform_email_verifications(verification_code);

-- Create a trigger to automatically update the updated_at column
CREATE TRIGGER trigger_update_platform_email_verifications_updated_at
BEFORE UPDATE ON platform_email_verifications
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();
