-- Codes that let another device join an account, merging its own into it.
-- Each works once, until it expires.
CREATE TABLE account_invites (
    code TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    -- The key that showed the code, whose name the joining phone sees.
    endpoint_id TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX account_invites_user ON account_invites (user_id);
