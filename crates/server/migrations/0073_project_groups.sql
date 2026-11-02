-- Folders a user lists projects under in place of their org, matched on
-- every machine with the home folder and a leading Projects/ left out.
CREATE TABLE project_groups (
    id UUID PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    -- As `groups::project_group_path` gives them, such as work.
    paths TEXT[] NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX project_groups_user ON project_groups (user_id, created_at);
