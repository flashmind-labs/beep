-- Ways of doing things Beep learned from how a user, or a group, asks for
-- them. Each belongs to exactly one of the two.
CREATE TABLE skills (
    id UUID PRIMARY KEY,
    user_id TEXT REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    group_id UUID REFERENCES chats (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    body TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    uses INTEGER NOT NULL DEFAULT 0,
    last_used TIMESTAMPTZ,
    -- The user changed it last, rather than Beep.
    edited BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((user_id IS NULL) <> (group_id IS NULL))
);
CREATE UNIQUE INDEX skills_user_name ON skills (user_id, lower(name)) WHERE user_id IS NOT NULL;
CREATE UNIQUE INDEX skills_group_name ON skills (group_id, lower(name)) WHERE group_id IS NOT NULL;

-- What a skill said before each change, newest with the highest seq.
CREATE TABLE skill_versions (
    skill_id UUID NOT NULL REFERENCES skills (id) ON DELETE CASCADE,
    seq BIGINT GENERATED ALWAYS AS IDENTITY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    body TEXT NOT NULL,
    edited BOOLEAN NOT NULL,
    PRIMARY KEY (skill_id, seq)
);

-- How many turns a chat had since its last skill review, and when that
-- review was, so the next one reads only what came after. No review yet
-- leaves `since` NULL.
CREATE TABLE skill_reviews (
    chat_id UUID PRIMARY KEY REFERENCES chats (id) ON DELETE CASCADE,
    turns INTEGER NOT NULL DEFAULT 0,
    since TIMESTAMPTZ
);
