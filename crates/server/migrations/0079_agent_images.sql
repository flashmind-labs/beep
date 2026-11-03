-- Copies of the pictures coding agent sessions show, by the `image_key` of
-- their bytes, so they show while their machine is offline. Nodes send them
-- as sessions show them, and the server keeps one it read from a node. The
-- bytes are a file without a chat: it expires after 30 days like any other,
-- and its row here goes with it.
CREATE TABLE agent_images (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    key TEXT NOT NULL,
    file_id TEXT NOT NULL REFERENCES files (id) ON DELETE CASCADE,
    -- Where it was first seen, for the record.
    machine_id TEXT NOT NULL,
    session TEXT NOT NULL,
    PRIMARY KEY (user_id, key)
);
CREATE INDEX agent_images_file ON agent_images (file_id);
