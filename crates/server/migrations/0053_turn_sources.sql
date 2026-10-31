-- The web pages a turn's reply used, which the apps show as chips under it:
-- pages Beep read first, then search results, by `position`.
CREATE TABLE turn_sources (
    message_id BIGINT NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    url TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (message_id, position)
);
