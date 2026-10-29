-- A prompt said aloud in live mode or as a held reply. The apps show a run
-- of spoken turns as one transcript; the text is the same as typed.
ALTER TABLE messages ADD COLUMN spoken BOOLEAN NOT NULL DEFAULT FALSE;
