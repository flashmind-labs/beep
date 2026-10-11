-- Settings an admin changes for the whole server, such as whether agents
-- are on.
CREATE TABLE server_settings (
    key TEXT PRIMARY KEY,
    value JSONB NOT NULL,
    -- The admin who last changed it.
    updated_by TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The agent a turn ran as, when agents were on.
ALTER TABLE turns ADD COLUMN agent TEXT;
