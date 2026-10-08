-- Each member pins a group for themselves, like a chat.
ALTER TABLE group_members ADD COLUMN pinned BOOLEAN NOT NULL DEFAULT FALSE;
