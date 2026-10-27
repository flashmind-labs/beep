-- A research run in a session: a lead agent plans it in the chat, then a
-- team of agents researches it in the background (see `crate::research`).
CREATE TABLE researches (
    id UUID PRIMARY KEY,
    chat_id UUID NOT NULL UNIQUE REFERENCES chats(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    topic TEXT NOT NULL,
    -- planning, researching, reviewing, writing, done, failed, stopped or
    -- interrupted.
    status TEXT NOT NULL DEFAULT 'planning',
    -- The approved plan: the questions and the model each agent runs on.
    plan JSONB,
    draft TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);
CREATE INDEX researches_user ON researches (user_id);

-- What the agents said to each other, the tools they ran and their notes.
CREATE TABLE research_entries (
    seq BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    research_id UUID NOT NULL REFERENCES researches(id) ON DELETE CASCADE,
    agent TEXT NOT NULL,
    -- said, step or note.
    kind TEXT NOT NULL,
    -- A message's addressee; NULL for the whole room.
    recipient TEXT,
    content TEXT NOT NULL,
    success BOOLEAN NOT NULL DEFAULT TRUE,
    -- A note's cited source numbers.
    sources INTEGER[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX research_entries_research ON research_entries (research_id, seq);

CREATE TABLE research_sources (
    research_id UUID NOT NULL REFERENCES researches(id) ON DELETE CASCADE,
    number INTEGER NOT NULL,
    url TEXT NOT NULL,
    title TEXT NOT NULL,
    quote TEXT NOT NULL,
    agent TEXT NOT NULL,
    PRIMARY KEY (research_id, number),
    UNIQUE (research_id, url)
);
