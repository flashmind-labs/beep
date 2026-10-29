-- Curation runs per memory scope and only when a scope needs it, so the
-- state is kept per scope: '' is the user's own, else `Scope::stored`.
-- `size` is how many memories the scope had after its last curation;
-- `close` and `crowded` count the signals seen since.
ALTER TABLE memory_curation
    ADD COLUMN scope TEXT NOT NULL DEFAULT '',
    ADD COLUMN size INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN close INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN crowded INTEGER NOT NULL DEFAULT 0,
    ALTER COLUMN curated_at DROP NOT NULL;
ALTER TABLE memory_curation DROP CONSTRAINT memory_curation_pkey;
ALTER TABLE memory_curation ADD PRIMARY KEY (user_id, scope);
