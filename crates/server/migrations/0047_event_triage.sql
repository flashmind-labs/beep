-- What the triage agent decided about each batch of account events (see
-- `crate::events::triage`), so the agent can say what came in and topics
-- can be tuned.
CREATE TABLE event_triage (
    id BIGSERIAL PRIMARY KEY,
    user_id TEXT NOT NULL,
    decided_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- inform, ignore or failed.
    decision TEXT NOT NULL,
    -- now or later, for inform.
    urgency TEXT,
    -- What the main agent is told, for inform.
    summary TEXT,
    -- Why it informs or ignores, or why the run failed.
    reason TEXT NOT NULL,
    -- The events the summary is about.
    refs BIGINT[] NOT NULL DEFAULT '{}',
    -- When an inform reached the main agent, in a wake or a conversation.
    delivered_at TIMESTAMPTZ,
    -- The wake it went out in.
    run_id TEXT
);
CREATE INDEX event_triage_user ON event_triage (user_id, decided_at);

-- The triage decision that covered the event.
ALTER TABLE app_events ADD COLUMN triage_id BIGINT;
