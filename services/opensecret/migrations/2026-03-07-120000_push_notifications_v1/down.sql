DROP TRIGGER IF EXISTS update_notification_deliveries_updated_at ON notification_deliveries;
DROP TRIGGER IF EXISTS update_push_devices_updated_at ON push_devices;

DROP INDEX IF EXISTS idx_notification_deliveries_pending;
DROP INDEX IF EXISTS idx_notification_events_user_due;
DROP INDEX IF EXISTS idx_push_devices_user_active;

DROP TABLE IF EXISTS notification_deliveries;
DROP TABLE IF EXISTS notification_events;
DROP TABLE IF EXISTS push_devices;
