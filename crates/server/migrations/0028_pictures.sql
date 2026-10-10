-- Pictures users send with a message. They stay as long as their chat:
-- `turn_at` names the session turn (its user message, in milliseconds),
-- `group_seq` the group message, and `session_id` holds the group's id.
-- Rows with either set are not dropped after 30 days.
ALTER TABLE files
    ADD COLUMN turn_at BIGINT,
    ADD COLUMN group_seq BIGINT,
    ADD COLUMN position INT NOT NULL DEFAULT 0,
    ADD COLUMN width INT NOT NULL DEFAULT 0,
    ADD COLUMN height INT NOT NULL DEFAULT 0;
CREATE INDEX files_session ON files (session_id);
