-- The group a reminder posts into, NULL for a user's own routines. The user
-- is who set it, whose timezone it keeps. It goes when the group does.
ALTER TABLE crons ADD COLUMN group_id TEXT REFERENCES groups (id) ON DELETE CASCADE;
CREATE INDEX crons_group ON crons (group_id) WHERE group_id IS NOT NULL;
