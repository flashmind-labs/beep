-- What coding agent sessions spent as the agents' own logs on the user's
-- machines tell it, sessions started outside Beep included, by session,
-- hour and model. A node reports a session whole from a start hour on, so
-- a report replaces the rows it covers rather than adding to them.
CREATE TABLE agent_session_usage (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    -- The node's key.
    endpoint_id TEXT NOT NULL,
    agent TEXT NOT NULL,
    -- The agent's own id for the session.
    session TEXT NOT NULL,
    hour TIMESTAMPTZ NOT NULL,
    model TEXT NOT NULL,
    -- The folder it ran in, which names its project.
    cwd TEXT NOT NULL,
    input_tokens BIGINT NOT NULL DEFAULT 0,
    output_tokens BIGINT NOT NULL DEFAULT 0,
    cache_read_tokens BIGINT NOT NULL DEFAULT 0,
    cache_write_tokens BIGINT NOT NULL DEFAULT 0,
    -- At API prices, of the priced tokens.
    cost_usd DOUBLE PRECISION NOT NULL DEFAULT 0,
    -- Tokens of models the node had no price for.
    unpriced_tokens BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, endpoint_id, agent, session, hour, model)
);
CREATE INDEX agent_session_usage_hour ON agent_session_usage (user_id, hour);

-- The agent's own id for a session Beep ran, so a session also read from
-- the logs counts once.
ALTER TABLE agent_usage ADD COLUMN agent_session TEXT;
