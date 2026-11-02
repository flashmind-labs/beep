-- A chat about one pull request or issue of a project: it runs as the
-- project agent with the thread read again each turn. One per thread.
CREATE TABLE project_thread_chats (
    project_id UUID NOT NULL REFERENCES projects ON DELETE CASCADE,
    -- `org/repo`, lowercase.
    repo TEXT NOT NULL,
    number BIGINT NOT NULL,
    -- `pull_request` or `issue`.
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    chat_id UUID NOT NULL UNIQUE REFERENCES chats (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, repo, number)
);

-- Replies Beep posted on a thread someone else opened: it gets one at most.
CREATE TABLE project_thread_replies (
    project_id UUID NOT NULL REFERENCES projects ON DELETE CASCADE,
    repo TEXT NOT NULL,
    number BIGINT NOT NULL,
    replied_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, repo, number)
);
