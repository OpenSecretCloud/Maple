-- This file should undo anything in `up.sql`
ALTER TABLE users ADD COLUMN seed_enc BYTEA;
