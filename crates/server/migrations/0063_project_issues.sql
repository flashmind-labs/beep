-- Open issues on a project's host the user chose "Not now" for: hidden in
-- Beep for that project, still open on the host.
CREATE TABLE project_dismissed_issues (
    project_id UUID NOT NULL REFERENCES projects ON DELETE CASCADE,
    number BIGINT NOT NULL,
    dismissed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, number)
);
