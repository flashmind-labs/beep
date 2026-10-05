-- Tokens and cost of every model call, summed per user, UTC day and model.
CREATE TABLE token_usage (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    day DATE NOT NULL,
    model TEXT NOT NULL,
    calls BIGINT NOT NULL DEFAULT 0,
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    cached_tokens BIGINT NOT NULL DEFAULT 0,
    cache_write_tokens BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    -- USD from the provider's list prices; calls to an unpriced model add nothing.
    cost DOUBLE PRECISION NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, day, model)
);
