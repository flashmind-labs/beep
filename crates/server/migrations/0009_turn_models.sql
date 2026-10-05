-- The model that answered each turn, keyed like memory_scores by the time of
-- the turn's user message in milliseconds.
CREATE TABLE turn_models (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    turn_at BIGINT NOT NULL,
    model TEXT NOT NULL,
    PRIMARY KEY (user_id, session_id, turn_at)
);
