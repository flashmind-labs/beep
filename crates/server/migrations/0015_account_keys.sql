-- Keys linked to an account besides the one that made it, such as a
-- computer the phone approved. A revoked key can no longer sign in.
CREATE TABLE account_keys (
    endpoint_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ
);
CREATE INDEX idx_account_keys_user ON account_keys (user_id);
