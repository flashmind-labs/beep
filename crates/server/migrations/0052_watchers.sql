-- How far each watcher has read each chat: the last message id it reviewed,
-- so nothing is read twice and a restart picks up where it stopped. A
-- watcher that failed keeps its place and counts the failures, so it waits
-- longer before it tries the same messages again.
CREATE TABLE watcher_reviews (
    chat_id UUID NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
    -- 'memory', 'skills' or 'improvement'.
    watcher TEXT NOT NULL,
    reviewed_seq BIGINT NOT NULL DEFAULT 0,
    reviewed_at TIMESTAMPTZ,
    failures INTEGER NOT NULL DEFAULT 0,
    failed_at TIMESTAMPTZ,
    PRIMARY KEY (chat_id, watcher)
);

-- Where Beep failed a user, as the improvement watcher proposes it. The
-- same problem seen in other chats counts towards one finding by its
-- fingerprint. Nothing here is applied: an admin resolves or dismisses it.
CREATE TABLE findings (
    id UUID PRIMARY KEY,
    fingerprint TEXT NOT NULL UNIQUE,
    -- 'correction', 'failed_tool', 'wrong_tool', 'could_have_found', 'loop',
    -- 'slow', 'gave_up' or 'other'.
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    steps TEXT NOT NULL,
    -- 'prompt', 'tool_schema', 'missing_tool', 'model' or 'other'.
    cause TEXT NOT NULL,
    cause_detail TEXT NOT NULL DEFAULT '',
    fix TEXT NOT NULL,
    -- 'open', 'resolved' or 'dismissed'.
    status TEXT NOT NULL DEFAULT 'open',
    seen INTEGER NOT NULL DEFAULT 1,
    first_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX findings_status ON findings (status, last_seen DESC);

-- Each time a finding was seen: in which chat and between which messages.
CREATE TABLE finding_sightings (
    finding_id UUID NOT NULL REFERENCES findings (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    chat_id UUID NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
    first_seq BIGINT NOT NULL,
    last_seq BIGINT NOT NULL,
    seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (finding_id, chat_id, first_seq)
);
CREATE INDEX finding_sightings_user ON finding_sightings (user_id, seen_at DESC);
