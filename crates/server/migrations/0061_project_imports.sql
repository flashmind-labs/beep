-- Repositories the user imported onto one of their machines, so they list
-- as projects. 0060 is left for the projects store on another branch.
CREATE TABLE project_imports (
    user_id TEXT NOT NULL,
    -- org/repo.
    project TEXT NOT NULL,
    -- The node's key, as `nodes.endpoint_id`.
    endpoint_id TEXT NOT NULL,
    -- Its absolute folder on that machine.
    folder TEXT NOT NULL,
    -- The repository's web page, such as https://github.com/org/repo.
    url TEXT NOT NULL,
    imported_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, project, endpoint_id)
);
