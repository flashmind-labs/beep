-- A user's settings for one coding agent harness on one machine, from the
-- Coding agents page. Empty text means what the machine finds. `env` holds
-- the harness's env vars as JSON, sealed with the server's credentials key
-- like a credential (nonce then ciphertext), since values may be secrets;
-- `env_names` lists their names so the apps can show them unopened.
CREATE TABLE harness_settings (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    machine_id TEXT NOT NULL,
    agent TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    program TEXT NOT NULL DEFAULT '',
    home TEXT NOT NULL DEFAULT '',
    args TEXT[] NOT NULL DEFAULT '{}',
    env_names TEXT[] NOT NULL DEFAULT '{}',
    env BYTEA,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, machine_id, agent)
);
