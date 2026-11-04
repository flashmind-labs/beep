-- Hosted sign-in. A code is single use and tied to the key that asked for it;
-- only its hash is kept.
CREATE TABLE login_codes (
    code_hash BYTEA PRIMARY KEY,
    endpoint_id TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ
);
CREATE INDEX login_codes_endpoint ON login_codes (endpoint_id);
CREATE INDEX login_codes_expires ON login_codes (expires_at);

-- The website's stable login ids, each on one account.
CREATE TABLE user_logins (
    sub TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    email TEXT,
    name TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX user_logins_user ON user_logins (user_id);
