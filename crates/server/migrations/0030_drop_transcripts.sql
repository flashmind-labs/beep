-- The server filled `messages` from every session's transcript before
-- applying this, so the transcript and the tables it was read with go. A
-- session that was never compacted keeps its transcript as its context.
UPDATE sessions SET context = conversation WHERE context IS NULL;
ALTER TABLE sessions ALTER COLUMN context SET NOT NULL;
ALTER TABLE sessions DROP COLUMN conversation, DROP COLUMN messages_backfilled;
DROP TABLE tool_failures;
DROP TABLE memory_scores;
DROP TABLE turn_models;
ALTER TABLE files DROP COLUMN turn_at;
