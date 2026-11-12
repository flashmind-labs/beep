CREATE TABLE IF NOT EXISTS projects (name TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS places (
    id INTEGER PRIMARY KEY,
    project TEXT NOT NULL REFERENCES projects(name) ON DELETE CASCADE,
    machine TEXT NOT NULL,
    folder TEXT NOT NULL,
    UNIQUE(project, machine, folder)
);
-- These locations prevent a stale scan from restoring a removed project's sessions.
CREATE TABLE IF NOT EXISTS retired_places (
    project TEXT NOT NULL, machine TEXT NOT NULL, folder TEXT NOT NULL,
    PRIMARY KEY(project, machine, folder)
);
CREATE INDEX IF NOT EXISTS places_machine ON places(machine);
CREATE INDEX IF NOT EXISTS retired_places_machine ON retired_places(machine);
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    stamp TEXT NOT NULL,
    data TEXT NOT NULL,
    activity INTEGER NOT NULL,
    place INTEGER REFERENCES places(id) ON DELETE CASCADE,
    -- Transcript entries before this index are indexed; NULL for a session
    -- the full reader of schema 1 indexed without keeping the count.
    done INTEGER DEFAULT 0,
    -- A walk back from walk_top toward done that has reached walk_low.
    walk_top INTEGER,
    walk_low INTEGER
);
CREATE INDEX IF NOT EXISTS sessions_place ON sessions(place);
CREATE TABLE IF NOT EXISTS removed (id TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS prompt_texts (
    id INTEGER PRIMARY KEY,
    session TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    words TEXT NOT NULL,
    pending INTEGER NOT NULL DEFAULT 0,
    UNIQUE(session, text)
);
CREATE VIRTUAL TABLE IF NOT EXISTS prompt_search USING fts5(
    words, content='prompt_texts', content_rowid='id',
    tokenize='trigram case_sensitive 1'
);
CREATE TRIGGER IF NOT EXISTS prompt_insert AFTER INSERT ON prompt_texts BEGIN
    INSERT INTO prompt_search(rowid, words) VALUES (new.id, new.words);
END;
CREATE TRIGGER IF NOT EXISTS prompt_delete AFTER DELETE ON prompt_texts BEGIN
    INSERT INTO prompt_search(prompt_search, rowid, words) VALUES ('delete', old.id, old.words);
END;
CREATE TRIGGER IF NOT EXISTS prompt_update AFTER UPDATE OF words ON prompt_texts BEGIN
    INSERT INTO prompt_search(prompt_search, rowid, words) VALUES ('delete', old.id, old.words);
    INSERT INTO prompt_search(rowid, words) VALUES (new.id, new.words);
END;
