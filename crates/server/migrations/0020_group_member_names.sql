-- Beep tells members apart by name, so two in one group may not share one,
-- even when they join at the same moment.
CREATE UNIQUE INDEX idx_group_members_name ON group_members (group_id, lower(name));
