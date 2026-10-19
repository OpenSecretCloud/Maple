-- Short-lived, root-key encrypted remote discovery/admission records. This is
-- not a device-grant store; grants and one-attempt pairing windows live locally.
CREATE TABLE remote_enrollment_records (
    user_id UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    project_id INTEGER NOT NULL REFERENCES org_projects(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('device', 'window', 'admission')),
    record_id TEXT NOT NULL CHECK (octet_length(record_id) <= 128),
    nonce UUID NOT NULL,
    expires_at BIGINT NOT NULL,
    ciphertext BYTEA NOT NULL CHECK (octet_length(ciphertext) <= 16384),
    PRIMARY KEY (user_id, kind, record_id)
);
CREATE INDEX remote_enrollment_expiry ON remote_enrollment_records (expires_at);
CREATE INDEX remote_enrollment_endpoint ON remote_enrollment_records (record_id) WHERE kind = 'device';
