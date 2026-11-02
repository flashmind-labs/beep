-- Projects the user pinned to the top of the sidebar. Apart from
-- `pinned_projects`, which keeps a project off sync.
CREATE TABLE projects_on_top (
    user_id TEXT NOT NULL,
    -- org/repo.
    project TEXT NOT NULL,
    pinned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, project)
);
