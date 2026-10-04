-- Every iroh key that has connected. `user_id` owns the rows in the other
-- tables; a new key gets its own endpoint id as user id. `name` is the
-- readable name made from the key, such as mega-fox-trot.
CREATE TABLE users (
    endpoint_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_users_user ON users (user_id);
