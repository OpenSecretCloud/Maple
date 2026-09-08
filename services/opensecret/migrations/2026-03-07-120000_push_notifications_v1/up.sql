CREATE TABLE push_devices (
    id BIGSERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT uuid_generate_v4() UNIQUE,
    user_id UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    installation_id UUID NOT NULL,
    platform TEXT NOT NULL CHECK (platform IN ('ios', 'android')),
    provider TEXT NOT NULL CHECK (provider IN ('apns', 'fcm')),
    environment TEXT NOT NULL CHECK (environment IN ('dev', 'prod')),
    app_id TEXT NOT NULL,
    push_token_enc BYTEA NOT NULL,
    push_token_hash BYTEA NOT NULL,
    notification_public_key BYTEA NOT NULL,
    key_algorithm TEXT NOT NULL CHECK (key_algorithm IN ('p256_ecdh_v1')),
    supports_encrypted_preview BOOLEAN NOT NULL DEFAULT false,
    supports_background_processing BOOLEAN NOT NULL DEFAULT false,
    last_seen_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    revoked_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (
        (platform = 'ios' AND provider = 'apns') OR
        (platform = 'android' AND provider = 'fcm')
    )
);

CREATE TABLE notification_events (
    id BIGSERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT uuid_generate_v4() UNIQUE,
    project_id INTEGER NOT NULL REFERENCES org_projects(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    delivery_mode TEXT NOT NULL CHECK (delivery_mode IN ('generic', 'encrypted_preview')),
    priority TEXT NOT NULL DEFAULT 'normal' CHECK (priority IN ('normal', 'high')),
    collapse_key TEXT,
    fallback_title TEXT NOT NULL,
    fallback_body TEXT NOT NULL,
    payload_enc BYTEA,
    not_before_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    cancelled_at TIMESTAMP WITH TIME ZONE
);

CREATE TABLE notification_deliveries (
    id BIGSERIAL PRIMARY KEY,
    event_id BIGINT NOT NULL REFERENCES notification_events(id) ON DELETE CASCADE,
    push_device_id BIGINT NOT NULL REFERENCES push_devices(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (
        status IN ('pending', 'leased', 'sent', 'retry', 'failed', 'invalid_token', 'cancelled')
    ),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    lease_owner TEXT,
    lease_expires_at TIMESTAMP WITH TIME ZONE,
    provider_message_id TEXT,
    provider_status_code INTEGER,
    last_error TEXT,
    sent_at TIMESTAMP WITH TIME ZONE,
    invalidated_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (event_id, push_device_id)
);

CREATE INDEX idx_push_devices_user_active
    ON push_devices(user_id, revoked_at);

CREATE UNIQUE INDEX idx_push_devices_installation_active
    ON push_devices(installation_id, environment)
    WHERE revoked_at IS NULL;

CREATE UNIQUE INDEX idx_push_devices_token_active
    ON push_devices(provider, environment, push_token_hash)
    WHERE revoked_at IS NULL;

CREATE INDEX idx_notification_events_user_due
    ON notification_events(user_id, not_before_at, cancelled_at);

CREATE INDEX idx_notification_deliveries_pending
    ON notification_deliveries(status, next_attempt_at);

CREATE TRIGGER update_push_devices_updated_at
    BEFORE UPDATE ON push_devices
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_notification_deliveries_updated_at
    BEFORE UPDATE ON notification_deliveries
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();
