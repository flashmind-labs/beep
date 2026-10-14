-- The prompt a run saved when it started, so other devices see it while the
-- run is still going. The finished turn replaces it.
ALTER TABLE runs ADD COLUMN prompt_id BIGINT REFERENCES messages (id) ON DELETE SET NULL;
