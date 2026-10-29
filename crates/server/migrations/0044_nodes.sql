-- Machines running `beep-node` that Beep runs work on (see `crate::nodes`).
-- A node signs in with one of the account's keys.
CREATE TABLE nodes (
    endpoint_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    -- The latest `EnvReport`, as JSON.
    env JSONB,
    connected_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- When it last connected or reported; it is online while connected.
    seen_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX nodes_user ON nodes (user_id);
