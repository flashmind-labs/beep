-- Which machines keep a project's folder in step, and where it is on each.
-- The nodes sync it between themselves; the server only tells them which
-- projects, folders and peers.
CREATE TABLE project_sync (
    user_id TEXT NOT NULL,
    project TEXT NOT NULL,
    -- The node's key, as `nodes.endpoint_id`.
    endpoint_id TEXT NOT NULL,
    -- Its absolute folder on that machine.
    folder TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, project, endpoint_id)
);

-- Projects kept on the machines they are on: no machine syncs them.
CREATE TABLE pinned_projects (
    user_id TEXT NOT NULL,
    project TEXT NOT NULL,
    pinned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, project)
);
