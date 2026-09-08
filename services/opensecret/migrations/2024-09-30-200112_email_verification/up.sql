-- Create the email_verifications table
CREATE TABLE email_verifications (
    id SERIAL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(uuid),
    verification_code UUID NOT NULL DEFAULT uuid_generate_v4(),
    is_verified BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL
);

-- Create indexes for faster lookups
CREATE INDEX idx_email_verifications_user_id ON email_verifications(user_id);
CREATE INDEX idx_email_verifications_verification_code ON email_verifications(verification_code);

-- Create a trigger to automatically update the updated_at column
CREATE OR REPLACE FUNCTION update_email_verifications_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_update_email_verifications_updated_at
BEFORE UPDATE ON email_verifications
FOR EACH ROW
EXECUTE FUNCTION update_email_verifications_updated_at();
