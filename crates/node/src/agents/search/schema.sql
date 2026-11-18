-- A new file gets a new epoch: a copy made from another file starts over.
CREATE TABLE IF NOT EXISTS meta (epoch TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sessions (
    -- Rows refer to this short key, not the long id.
    key INTEGER PRIMARY KEY,
    -- The agent's own id for the session.
    id TEXT NOT NULL UNIQUE,
    -- The activity it was fully indexed at; '' while it is not.
    stamp TEXT NOT NULL DEFAULT '',
    -- Unix milliseconds of its last activity.
    activity INTEGER NOT NULL DEFAULT 0,
    -- Transcript entries before this index are indexed.
    done INTEGER NOT NULL DEFAULT 0,
    -- A walk back from walk_top toward done that has reached walk_low.
    walk_top INTEGER,
    walk_low INTEGER,
    -- Session details sent to search clients with an empty text row.
    metadata TEXT
);
-- The index is its own change log: copies apply rows in `seq` order. A row
-- with text adds a prompt or reply; one without is a reset, after which a
-- copy holds only the session's later rows.
CREATE TABLE IF NOT EXISTS log (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    session INTEGER NOT NULL REFERENCES sessions(key),
    text TEXT,
    -- Keeps a text unique in its session without a second copy of it.
    hash INTEGER,
    UNIQUE(session, hash)
);
-- Without positions the index is a third of the size. A search asks for
-- every trigram of a word, then `holds` checks each candidate's text.
CREATE VIRTUAL TABLE IF NOT EXISTS log_search USING fts5(
    text, content='log', content_rowid='seq',
    tokenize='trigram', detail=none
);
CREATE TRIGGER IF NOT EXISTS log_insert AFTER INSERT ON log WHEN new.text IS NOT NULL BEGIN
    INSERT INTO log_search(rowid, text) VALUES (new.seq, new.text);
END;
CREATE TRIGGER IF NOT EXISTS log_delete AFTER DELETE ON log WHEN old.text IS NOT NULL BEGIN
    INSERT INTO log_search(log_search, rowid, text) VALUES ('delete', old.seq, old.text);
END;
