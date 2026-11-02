-- Projects whose sync the user paused: every machine keeps its files and
-- its list, and nothing moves until the project is resumed.
CREATE TABLE paused_syncs (
    user_id TEXT NOT NULL,
    project TEXT NOT NULL,
    paused_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, project)
);
