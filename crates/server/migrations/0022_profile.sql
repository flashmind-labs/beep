-- The one-time read of each connected mailbox and calendar that builds the
-- user's profile. A mailbox is read a year at a time from the newest, and
-- `read_before` is where the next year starts, so a pass cut short resumes.
CREATE TABLE profile_reads (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    source TEXT NOT NULL,
    read_before TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    PRIMARY KEY (user_id, source)
);

-- What the read found, as short notes the profile is written from.
CREATE TABLE profile_notes (
    id BIGSERIAL PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    source TEXT NOT NULL,
    noted_on DATE,
    note TEXT NOT NULL
);
CREATE INDEX idx_profile_notes_user ON profile_notes (user_id, source);
