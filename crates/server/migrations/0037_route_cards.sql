-- The routes a get_route call found, by the call's id, which the apps draw
-- as a card. They go with the chat.
CREATE TABLE route_cards (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    call_id TEXT NOT NULL,
    chat_id UUID NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
    card JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, call_id)
);
CREATE INDEX route_cards_chat ON route_cards (chat_id);
