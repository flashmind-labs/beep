-- Actions the user allowed for the rest of one chat ("Allow for this
-- session"), by app and tool slug as `app_permissions.allowed` keeps them.
CREATE TABLE chat_allows (
    chat_id UUID NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    app TEXT NOT NULL,
    tool TEXT NOT NULL,
    allowed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, app, tool)
);
