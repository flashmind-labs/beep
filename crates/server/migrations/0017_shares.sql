-- A frozen copy of a session that anyone holding the token may read and
-- copy into their own account. Sharing the session again replaces the copy
-- and keeps the token.
CREATE TABLE shares (
    token TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    title TEXT NOT NULL,
    -- The shared turns as a conversation of user and assistant text only.
    conversation TEXT NOT NULL,
    shared_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, session_id)
);
