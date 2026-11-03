-- Webhooks Beep made on a project's repository for live updates (see
-- `repo_watch::forward`). A row stays until the node that made the webhook
-- says it is gone, so one a node left behind (it stopped before removing
-- it) is removed at the next watch of that repository. Beep removes only
-- the webhooks listed here, never one it did not make.
CREATE TABLE project_webhooks (
    id UUID PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    -- `github` or `forgejo:<host>`, as `projects.issues_host` keeps it.
    host TEXT NOT NULL,
    -- owner/name.
    repo TEXT NOT NULL,
    -- The node that made it, as `nodes.endpoint_id`.
    endpoint_id TEXT NOT NULL,
    -- The host's id of the webhook; NULL until the node says.
    hook_id BIGINT,
    -- Its secret, sealed with the credentials key.
    secret BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX project_webhooks_repo ON project_webhooks (user_id, host, repo);
