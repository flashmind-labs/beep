-- Things that happened in a user's connected accounts (see `crate::events`).
-- `dedupe_key` is the same for one happening however it arrived, so a
-- trigger and a poll of the same email store one row.
CREATE TABLE app_events (
    id BIGSERIAL PRIMARY KEY,
    user_id TEXT NOT NULL,
    app TEXT NOT NULL,
    kind TEXT NOT NULL,
    dedupe_key TEXT NOT NULL,
    summary TEXT NOT NULL,
    detail TEXT,
    -- What the agent acts on, such as a message or thread id.
    ids JSONB NOT NULL,
    -- The payload as it came, kept on the server only.
    raw JSONB NOT NULL,
    source TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- pending, woken, held or digested.
    state TEXT NOT NULL DEFAULT 'pending',
    run_id TEXT,
    UNIQUE (user_id, dedupe_key)
);
CREATE INDEX app_events_state ON app_events (user_id, state, received_at);

-- A user's choice for one event topic; topics without a row keep their default.
CREATE TABLE event_topics (
    user_id TEXT NOT NULL,
    topic TEXT NOT NULL,
    enabled BOOLEAN NOT NULL,
    PRIMARY KEY (user_id, topic)
);

-- Composio trigger instances made for a user, so turning a topic off can
-- disable them.
CREATE TABLE event_triggers (
    user_id TEXT NOT NULL,
    slug TEXT NOT NULL,
    account_id TEXT NOT NULL,
    trigger_id TEXT NOT NULL,
    PRIMARY KEY (user_id, slug, account_id)
);

-- Where each polled source of a user stopped.
CREATE TABLE event_polls (
    user_id TEXT NOT NULL,
    source TEXT NOT NULL,
    polled_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, source)
);
