-- The chat a routine was set up from, where its runs reply. NULL for routines
-- made outside a chat, which get a new chat on each run.
ALTER TABLE crons ADD COLUMN session_id TEXT;
