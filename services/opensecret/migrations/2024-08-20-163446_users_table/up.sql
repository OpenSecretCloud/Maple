CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT uuid_generate_v4() UNIQUE,
    name TEXT,
    email TEXT NOT NULL UNIQUE,
    password_enc BYTEA NOT NULL,
    seed_enc BYTEA
);

-- Add an index on the uuid column
CREATE INDEX idx_users_uuid ON users(uuid);
