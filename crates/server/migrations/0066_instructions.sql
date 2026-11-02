-- The user's instructions for every agent Beep runs, one row per saved
-- version; the newest is the one in use.
CREATE TABLE instructions_versions (
    user_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    text TEXT NOT NULL,
    -- What made it, such as "Restored version 3"; empty for an edit.
    note TEXT NOT NULL DEFAULT '',
    saved_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, version)
);
