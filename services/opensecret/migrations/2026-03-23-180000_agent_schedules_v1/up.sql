CREATE TABLE agent_schedules (
    id BIGSERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    user_id UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    agent_id BIGINT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    description TEXT NOT NULL,
    instruction_enc BYTEA NOT NULL,
    schedule_kind TEXT NOT NULL CHECK (schedule_kind IN ('one_off', 'recurring')),
    recurrence_type TEXT CHECK (
        recurrence_type IS NULL OR recurrence_type IN ('interval', 'daily', 'weekly')
    ),
    schedule_spec JSONB NOT NULL,
    timezone_mode TEXT NOT NULL CHECK (timezone_mode IN ('follow_user', 'fixed')),
    resolved_timezone TEXT NOT NULL,
    fixed_timezone TEXT,
    stale_after_minutes INTEGER NOT NULL DEFAULT 15 CHECK (stale_after_minutes > 0),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'completed', 'cancelled')),
    next_scheduled_for TIMESTAMP WITH TIME ZONE,
    last_scheduled_for TIMESTAMP WITH TIME ZONE,
    last_run_at TIMESTAMP WITH TIME ZONE,
    run_count INTEGER NOT NULL DEFAULT 0 CHECK (run_count >= 0),
    cancelled_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT agent_schedules_fixed_timezone_check CHECK (
        (timezone_mode = 'follow_user' AND fixed_timezone IS NULL)
        OR (timezone_mode = 'fixed' AND fixed_timezone IS NOT NULL)
    ),
    CONSTRAINT agent_schedules_kind_recurrence_check CHECK (
        (schedule_kind = 'one_off' AND recurrence_type IS NULL)
        OR (schedule_kind = 'recurring' AND recurrence_type IS NOT NULL)
    )
);

CREATE TABLE agent_schedule_runs (
    id BIGSERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    schedule_id BIGINT NOT NULL REFERENCES agent_schedules(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    agent_id BIGINT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    scheduled_for TIMESTAMP WITH TIME ZONE NOT NULL,
    stale_after_at TIMESTAMP WITH TIME ZONE NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (
        status IN ('pending', 'leased', 'retry', 'completed', 'failed', 'cancelled', 'expired')
    ),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    next_attempt_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    lease_owner TEXT,
    lease_expires_at TIMESTAMP WITH TIME ZONE,
    started_at TIMESTAMP WITH TIME ZONE,
    first_output_at TIMESTAMP WITH TIME ZONE,
    first_message_id UUID,
    output_count INTEGER NOT NULL DEFAULT 0 CHECK (output_count >= 0),
    notification_enqueued_at TIMESTAMP WITH TIME ZONE,
    completed_at TIMESTAMP WITH TIME ZONE,
    last_error TEXT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (schedule_id, scheduled_for)
);

CREATE INDEX idx_agent_schedules_active_due
    ON agent_schedules(status, next_scheduled_for, id)
    WHERE status = 'active';

CREATE INDEX idx_agent_schedules_agent_status
    ON agent_schedules(agent_id, status, next_scheduled_for);

CREATE INDEX idx_agent_schedules_user_status
    ON agent_schedules(user_id, status, created_at DESC);

CREATE INDEX idx_agent_schedule_runs_pending
    ON agent_schedule_runs(status, next_attempt_at, id);

CREATE INDEX idx_agent_schedule_runs_schedule
    ON agent_schedule_runs(schedule_id, scheduled_for DESC);

CREATE INDEX idx_agent_schedule_runs_agent_status
    ON agent_schedule_runs(agent_id, status, scheduled_for DESC);

CREATE TRIGGER update_agent_schedules_updated_at
    BEFORE UPDATE ON agent_schedules
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_agent_schedule_runs_updated_at
    BEFORE UPDATE ON agent_schedule_runs
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();
