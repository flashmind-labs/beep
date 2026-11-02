-- A project's terminals: commands a node keeps running in the background
-- (`process/1`), named and kept per project. How each stands is asked of
-- its node.
CREATE TABLE terminals (
    id UUID PRIMARY KEY,
    user_id TEXT NOT NULL,
    -- org/repo.
    project TEXT NOT NULL,
    -- The node's key, as `nodes.endpoint_id`.
    endpoint_id TEXT NOT NULL,
    -- The node's number for the task.
    task_id BIGINT NOT NULL,
    name TEXT NOT NULL,
    command TEXT NOT NULL,
    cwd TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX terminals_project ON terminals (user_id, project, started_at);
