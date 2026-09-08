CREATE TABLE enclave_secrets (
    id SERIAL PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    value BYTEA NOT NULL
);

-- Create an index on key for faster lookups
CREATE INDEX idx_enclave_secrets_key ON enclave_secrets(key);
