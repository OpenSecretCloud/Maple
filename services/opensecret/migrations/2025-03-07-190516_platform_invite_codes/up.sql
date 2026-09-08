-- Create platform_invite_codes table
CREATE TABLE platform_invite_codes (
    id SERIAL PRIMARY KEY,
    code UUID NOT NULL UNIQUE
);

-- Create an index on the code for faster lookups
CREATE INDEX platform_invite_codes_code_idx ON platform_invite_codes (code);
