-- The agent a call's turn ran as, so spend can be capped per agent later.
-- Empty for calls outside an agent's turn: Jev, memory, titles, routines
-- without an agent, and every turn while agents are off.
ALTER TABLE token_usage ADD COLUMN agent TEXT NOT NULL DEFAULT '';
ALTER TABLE token_usage DROP CONSTRAINT token_usage_pkey;
ALTER TABLE token_usage ADD PRIMARY KEY (user_id, day, model, agent);
