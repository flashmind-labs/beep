-- A repository the user works on through its own agent chat (see
-- `crate::projects`).
CREATE TABLE projects (
    id UUID PRIMARY KEY,
    user_id TEXT NOT NULL,
    -- owner/repo.
    name TEXT NOT NULL,
    -- Where its issues and its PRs go: `github` or `forgejo:<host>`; NULL
    -- keeps them in Beep only.
    issues TEXT,
    prs TEXT,
    -- public or private.
    visibility TEXT NOT NULL DEFAULT 'public',
    default_branch TEXT NOT NULL DEFAULT 'main',
    -- The endpoint id of the machine its workers start on first.
    pin TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, name)
);

-- Where the project is checked out. The home checkout gets the outputs.
CREATE TABLE project_checkouts (
    project_id UUID REFERENCES projects ON DELETE CASCADE,
    -- An endpoint id.
    machine TEXT NOT NULL,
    path TEXT NOT NULL,
    home BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (project_id, machine, path)
);

-- The project's agent chat, made on first use.
CREATE TABLE project_agents (
    project_id UUID PRIMARY KEY REFERENCES projects ON DELETE CASCADE,
    chat_id UUID NOT NULL UNIQUE REFERENCES chats(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One unit of work one worker can finish on one branch.
CREATE TABLE project_tasks (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    -- Its short name in the project, in its branch and folders.
    slug TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    -- planned, running, blocked, review, done or dropped.
    state TEXT NOT NULL DEFAULT 'planned',
    -- The task that must finish first.
    depends_on UUID REFERENCES project_tasks(id) ON DELETE SET NULL,
    -- The machine capabilities it needs, such as cuda or xcode.
    needs TEXT[] NOT NULL DEFAULT '{}',
    -- The issue mirroring it, on `github` or `forgejo:<host>`.
    issue_host TEXT,
    issue_number BIGINT,
    issue_url TEXT,
    branch TEXT,
    pr_url TEXT,
    -- How it was asked to start, kept while it waits for a slot.
    start JSONB,
    -- A fingerprint of the PR's checks and reviews as last reported, so a
    -- check only reports changes.
    pr_seen TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, slug)
);

-- A coding agent working on a task on one machine.
CREATE TABLE project_workers (
    id UUID PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    -- An endpoint id.
    machine TEXT NOT NULL,
    -- claude or codex.
    agent TEXT NOT NULL,
    -- The node's id for its session, once it has started.
    session TEXT,
    -- The worktree it writes in.
    folder TEXT NOT NULL,
    -- starting, running, waiting, ended or failed.
    state TEXT NOT NULL DEFAULT 'starting',
    -- Whether it waits on an answer to a question it asked.
    asking BOOLEAN NOT NULL DEFAULT false,
    last_output TEXT,
    persona TEXT,
    -- The commit the persona file was read at.
    persona_commit TEXT,
    -- How many of its session's turns the project chat has heard about.
    turns_seen INTEGER NOT NULL DEFAULT 0,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_turn_at TIMESTAMPTZ,
    ended_at TIMESTAMPTZ
);
-- The folder lock: one live worker per folder on a machine.
CREATE UNIQUE INDEX project_workers_folder ON project_workers (machine, folder)
    WHERE state IN ('starting', 'running', 'waiting');
-- One live worker per task.
CREATE UNIQUE INDEX project_workers_task ON project_workers (task_id)
    WHERE state IN ('starting', 'running', 'waiting');
CREATE INDEX project_workers_project ON project_workers (project_id, started_at);
CREATE INDEX project_workers_user ON project_workers (user_id)
    WHERE state IN ('starting', 'running', 'waiting');
