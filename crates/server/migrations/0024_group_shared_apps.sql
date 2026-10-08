-- Whether Beep may use a member's apps when someone else in the group asks.
ALTER TABLE group_members ADD COLUMN shares_apps BOOLEAN NOT NULL DEFAULT TRUE;
