-- Memories live in the flashmind memory store, in its own `memories` and
-- `memory_meta` tables, created by `MemoryStore::connect_postgres`.

CREATE TABLE sessions (
    user_id TEXT NOT NULL,
    id TEXT NOT NULL,
    -- Insertion order, which the session list follows.
    seq BIGINT GENERATED ALWAYS AS IDENTITY,
    -- The full transcript the phone shows.
    conversation TEXT NOT NULL,
    -- What the model sees, compacted once the session is long and idle.
    context TEXT,
    context_tokens BIGINT NOT NULL DEFAULT 0,
    pinned BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ,
    PRIMARY KEY (user_id, id)
);

CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    seq BIGINT GENERATED ALWAYS AS IDENTITY,
    user_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    message TEXT NOT NULL,
    status TEXT NOT NULL,
    UNIQUE (user_id, request_id)
);
CREATE UNIQUE INDEX one_active_run ON runs (user_id, session_id) WHERE status = 'running';
CREATE INDEX idx_runs_user_session ON runs (user_id, session_id, seq);

CREATE TABLE session_titles (
    user_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    title TEXT NOT NULL,
    manual BOOLEAN NOT NULL DEFAULT FALSE,
    titled_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, session_id)
);

CREATE TABLE tool_failures (
    user_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    call_id TEXT NOT NULL,
    PRIMARY KEY (user_id, session_id, call_id)
);

CREATE TABLE memory_scores (
    user_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    turn_at BIGINT NOT NULL,
    probability DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (user_id, session_id, turn_at)
);

CREATE TABLE memory_curation (
    user_id TEXT PRIMARY KEY,
    curated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE crons (
    user_id TEXT NOT NULL,
    id TEXT NOT NULL,
    label TEXT NOT NULL,
    prompt TEXT NOT NULL,
    schedule TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    run_once BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_run_at TIMESTAMPTZ,
    last_status TEXT,
    last_error TEXT,
    PRIMARY KEY (user_id, id)
);

CREATE TABLE cron_logs (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id TEXT NOT NULL,
    cron_id TEXT NOT NULL,
    run_id TEXT,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'running',
    error TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_cron_logs_user_cron ON cron_logs (user_id, cron_id);
CREATE INDEX idx_cron_logs_run ON cron_logs (run_id);

CREATE TABLE devices (
    user_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    fcm_token TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, device_id)
);

CREATE TABLE locations (
    user_id TEXT PRIMARY KEY,
    latitude DOUBLE PRECISION NOT NULL,
    longitude DOUBLE PRECISION NOT NULL,
    accuracy DOUBLE PRECISION NOT NULL,
    timezone TEXT NOT NULL DEFAULT 'UTC',
    place TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE mcp_connections (
    user_id TEXT NOT NULL,
    id TEXT NOT NULL,
    provider TEXT NOT NULL,
    label TEXT NOT NULL,
    config TEXT NOT NULL DEFAULT '{}',
    credentials TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'disconnected',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, id)
);

CREATE TABLE mcp_servers (
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    config JSONB NOT NULL DEFAULT '{}',
    credentials JSONB,
    PRIMARY KEY (user_id, name)
);

CREATE TABLE notifications (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    app TEXT NOT NULL,
    title TEXT,
    text TEXT,
    timestamp BIGINT NOT NULL,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, app, timestamp, title)
);
CREATE INDEX idx_notifications_user_ts ON notifications (user_id, timestamp DESC);
CREATE INDEX idx_notifications_ts ON notifications (timestamp);

CREATE TABLE habits (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    frequency TEXT NOT NULL DEFAULT 'daily',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    archived BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (user_id, name)
);

CREATE TABLE habit_logs (
    id TEXT PRIMARY KEY,
    habit_id TEXT NOT NULL REFERENCES habits (id),
    user_id TEXT NOT NULL,
    value DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    note TEXT,
    logged_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_habit_logs_habit ON habit_logs (habit_id, logged_at);

-- Sync state of mail and Slack. The names predate Slack.
CREATE TABLE mail_sync (
    user_id TEXT NOT NULL,
    mailbox TEXT NOT NULL,
    synced_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, mailbox)
);

CREATE TABLE mail_seen (
    user_id TEXT NOT NULL,
    mailbox TEXT NOT NULL,
    message_id TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, mailbox, message_id)
);
CREATE INDEX idx_mail_seen_received ON mail_seen (user_id, mailbox, received_at);
