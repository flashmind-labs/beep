-- The device that started a coding-agent session through Beep. The
-- session's alerts (it asks, it finished) go to that device only. A session
-- with no row here, such as one started in a terminal, by a routine or by a
-- project agent, alerts the user's computers.
CREATE TABLE agent_session_starts (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    -- The node's key.
    machine TEXT NOT NULL,
    agent TEXT NOT NULL,
    -- The agent's own id for the session.
    session TEXT NOT NULL,
    -- The key of the phone or computer that started it.
    device TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (machine, agent, session)
);
