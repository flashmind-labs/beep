-- A woken run that told the user something is an alert, not a chat: it is
-- left out of the chat list and search for the apps that ask, until the user
-- replies in it, which deletes its row here. The chat's member is its owner.
CREATE TABLE chat_alerts (
    chat_id UUID PRIMARY KEY REFERENCES chats (id) ON DELETE CASCADE,
    -- Where it came from, as the user reads it, such as "Places".
    source TEXT NOT NULL,
    -- What Beep told the user.
    text TEXT NOT NULL,
    read BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
