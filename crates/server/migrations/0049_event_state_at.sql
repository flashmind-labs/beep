-- When an event last changed state, so the sweep can tell a triage that
-- died from one still running.
ALTER TABLE app_events ADD COLUMN state_at TIMESTAMPTZ NOT NULL DEFAULT now();
