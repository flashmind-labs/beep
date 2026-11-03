-- What coding agent sessions on the user's machines spent, per session and
-- day (in the user's timezone), as their nodes report each finished turn.
-- `cost_usd` is the agent's own estimate at API prices, NULL while no turn
-- of the session that day reported one; `unpriced_tokens` are the tokens of
-- turns that reported none, such as Codex's.
CREATE TABLE agent_usage (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    -- The node's key.
    endpoint_id TEXT NOT NULL,
    -- The node's id for the session.
    session TEXT NOT NULL,
    day DATE NOT NULL,
    agent TEXT NOT NULL,
    -- The folder it runs in, which names its project.
    cwd TEXT NOT NULL,
    cost_usd DOUBLE PRECISION,
    tokens BIGINT NOT NULL DEFAULT 0,
    unpriced_tokens BIGINT NOT NULL DEFAULT 0,
    turns INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, endpoint_id, session, day)
);
CREATE INDEX agent_usage_day ON agent_usage (user_id, day);

-- The same tokens by model, when the agent names them.
CREATE TABLE agent_model_usage (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    day DATE NOT NULL,
    agent TEXT NOT NULL,
    model TEXT NOT NULL,
    tokens BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, day, agent, model)
);

-- How much of each agent's plan is used, the freshest report of any of the
-- user's machines: limits belong to the account, not the machine.
CREATE TABLE plan_usage (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    agent TEXT NOT NULL,
    endpoint_id TEXT NOT NULL,
    plan TEXT,
    -- [{minutes, used_percent, resets_at, scope}], resets_at in Unix seconds.
    windows JSONB NOT NULL,
    read_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, agent)
);
