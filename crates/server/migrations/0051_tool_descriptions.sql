-- What a tool step showed while it ran, such as "Recalling memories".
-- Empty for steps saved before it was kept.
ALTER TABLE tool_calls ADD COLUMN description TEXT NOT NULL DEFAULT '';
