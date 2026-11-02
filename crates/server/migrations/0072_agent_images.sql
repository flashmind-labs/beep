-- Images users sent to a coding agent session, by the name its log gives
-- them. The bytes are a file without a chat, so it expires after 30 days
-- like any other, and its rows here go with it.
CREATE TABLE agent_images (
    id BIGSERIAL PRIMARY KEY,
    file_id TEXT NOT NULL REFERENCES files (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    machine_id TEXT NOT NULL,
    -- The node's id for the session.
    session TEXT NOT NULL,
    name TEXT NOT NULL,
    -- `image_key` of the bytes, for a log that lost the name.
    key TEXT NOT NULL
);
CREATE INDEX agent_images_session ON agent_images (user_id, machine_id, session);
