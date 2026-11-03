-- How what a session spent was paid for: `subscription` (a Claude or
-- ChatGPT plan, Copilot), `metered` (API keys, OpenRouter, Bedrock), or
-- `unknown` from a node that could not tell or from before it said. One
-- hour of one model may be paid both ways, so it joins the key.
ALTER TABLE agent_session_usage ADD COLUMN billing TEXT NOT NULL DEFAULT 'unknown';
ALTER TABLE agent_session_usage DROP CONSTRAINT agent_session_usage_pkey;
ALTER TABLE agent_session_usage
    ADD PRIMARY KEY (user_id, endpoint_id, agent, session, hour, model, billing);
