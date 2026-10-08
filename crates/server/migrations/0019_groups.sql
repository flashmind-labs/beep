-- Group chats: people talk to each other, and Beep replies when someone
-- mentions @beep, running as that person.
CREATE TABLE groups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    -- The token in the invite link. Resetting the link replaces it.
    invite TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE group_members (
    group_id TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    -- What the others see, picked when creating or joining.
    name TEXT NOT NULL,
    creator BOOLEAN NOT NULL DEFAULT FALSE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (group_id, user_id)
);
CREATE INDEX idx_group_members_user ON group_members (user_id);

CREATE TABLE group_messages (
    group_id TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    seq BIGSERIAL,
    -- NULL for Beep's replies.
    author_id TEXT,
    -- Kept so the messages of someone who left still show their name.
    author_name TEXT NOT NULL,
    -- The client's id for the message, so a retry posts once.
    request_id TEXT,
    content TEXT NOT NULL,
    sent_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (group_id, seq),
    UNIQUE (group_id, request_id)
);
