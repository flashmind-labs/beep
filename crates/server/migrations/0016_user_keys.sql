-- An account's id no longer is its phone's key. Every key that signs in,
-- the phone's own included, is a row here, so an account can have several.
CREATE TABLE user_keys (
    endpoint_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    -- The key that made the account; it links the others.
    is_primary BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ
);
CREATE INDEX idx_user_keys_user ON user_keys (user_id);
CREATE UNIQUE INDEX idx_user_keys_primary ON user_keys (user_id) WHERE is_primary;

INSERT INTO user_keys (endpoint_id, user_id, name, kind, is_primary, created_at)
SELECT id, id, name, 'phone', true, created_at FROM users;
INSERT INTO user_keys (endpoint_id, user_id, name, kind, created_at, revoked_at)
SELECT endpoint_id, user_id, name, kind, created_at, revoked_at FROM account_keys;
DROP TABLE account_keys;

-- The id services outside the database know the account by, such as its
-- Composio entity and sandbox folder. Old accounts keep their old id there.
ALTER TABLE users ADD COLUMN external_id TEXT;
UPDATE users SET external_id = id;
ALTER TABLE users ALTER COLUMN external_id SET NOT NULL;

-- Push tokens belong to the key that registered them, since a notice is
-- sealed to that key.
ALTER TABLE devices ADD COLUMN endpoint_id TEXT;
UPDATE devices SET endpoint_id = user_id;
ALTER TABLE devices ALTER COLUMN endpoint_id SET NOT NULL;
CREATE INDEX idx_devices_endpoint ON devices (endpoint_id);

-- Every table's user_id follows through ON UPDATE CASCADE.
UPDATE users SET id = replace(gen_random_uuid()::text, '-', '');
