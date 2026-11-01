-- One-time setup tokens that link a machine to an account (`beep-node
-- setup`). Only a hash of each token's secret is kept.
CREATE TABLE node_tokens (
    hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    -- What to call the machine, when the user named it up front.
    name TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    -- The key that claimed it.
    endpoint_id TEXT
);
CREATE INDEX node_tokens_user ON node_tokens (user_id);
