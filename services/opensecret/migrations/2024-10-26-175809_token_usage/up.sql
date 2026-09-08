-- Create token_usage table
CREATE TABLE token_usage (
    id BIGSERIAL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    input_tokens INTEGER NOT NULL CHECK (input_tokens >= 0),
    output_tokens INTEGER NOT NULL CHECK (output_tokens >= 0),
    estimated_cost DECIMAL(12, 6) NOT NULL CHECK (estimated_cost >= 0),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create an index on user_id for faster lookups
CREATE INDEX idx_token_usage_user_id ON token_usage(user_id);

-- Create an index on created_at for time-based queries
CREATE INDEX idx_token_usage_created_at ON token_usage(created_at);
