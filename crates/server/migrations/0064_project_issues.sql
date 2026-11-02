-- Open issues on a project's host the user chose "Not now" for: hidden in
-- Beep for that project, still open on the host. Keyed by host, so moving
-- the project to another host does not hide that host's issues.
CREATE TABLE project_dismissed_issues (
    project_id UUID NOT NULL REFERENCES projects ON DELETE CASCADE,
    -- `github` or `forgejo:<host>`.
    host TEXT NOT NULL,
    number BIGINT NOT NULL,
    dismissed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, host, number)
);

-- A task tracking an issue someone else opened: Beep never closes,
-- relabels or edits it, and comments on it at most once, after asking.
ALTER TABLE project_tasks ADD COLUMN issue_foreign BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE project_tasks ADD COLUMN issue_commented BOOLEAN NOT NULL DEFAULT false;
