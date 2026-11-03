-- The chat about each update the user opened (`OpenUpdateChat`), so opening
-- it again shows the same chat. Goes with the update or the chat.
CREATE TABLE update_chats (
    triage_id BIGINT PRIMARY KEY REFERENCES event_triage (id) ON DELETE CASCADE,
    chat_id UUID NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX update_chats_chat ON update_chats (chat_id);
