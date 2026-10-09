-- Files Beep sent the user in a chat, such as a mail attachment. Deleted
-- with the session, or after 30 days.
CREATE TABLE files (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    name TEXT NOT NULL,
    mime TEXT NOT NULL,
    data BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX files_user ON files (user_id, created_at);
