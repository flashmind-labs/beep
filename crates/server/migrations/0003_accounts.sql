-- One account per iroh key: a user's id is its endpoint id, and every
-- table's `user_id` points at it. Restoring the key from a backup signs back
-- into the same account, so an account never needs a second key.

ALTER TABLE users RENAME TO old_keys;

-- `name` is the readable name made from the key, such as mega-fox-trot.
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Every id that owns rows becomes an account under its old id first.
INSERT INTO users (id, name)
SELECT user_id, user_id FROM (
    SELECT user_id FROM old_keys
    UNION SELECT user_id FROM sessions
    UNION SELECT user_id FROM runs
    UNION SELECT user_id FROM session_titles
    UNION SELECT user_id FROM tool_failures
    UNION SELECT user_id FROM memory_scores
    UNION SELECT user_id FROM memory_curation
    UNION SELECT user_id FROM crons
    UNION SELECT user_id FROM cron_logs
    UNION SELECT user_id FROM devices
    UNION SELECT user_id FROM locations
    UNION SELECT user_id FROM mcp_connections
    UNION SELECT user_id FROM mcp_servers
    UNION SELECT user_id FROM notifications
    UNION SELECT user_id FROM habits
    UNION SELECT user_id FROM habit_logs
    UNION SELECT user_id FROM mail_sync
    UNION SELECT user_id FROM mail_seen
) owners;

ALTER TABLE sessions ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE runs ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE session_titles ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE tool_failures ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE memory_scores ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE memory_curation ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE crons ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE cron_logs ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE devices ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE locations ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE mcp_connections ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE mcp_servers ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE notifications ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE habits ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE habit_logs ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE mail_sync ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE mail_seen ADD FOREIGN KEY (user_id) REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE;

-- Then each account a key signed into moves to that key's id, taking its
-- rows along. An account several keys shared goes to the oldest; the others
-- start fresh on their next sign-in.
CREATE TEMPORARY TABLE moves ON COMMIT DROP AS
SELECT DISTINCT ON (user_id) user_id AS old_id, endpoint_id AS new_id
FROM old_keys
WHERE user_id <> endpoint_id
  AND NOT EXISTS (SELECT 1 FROM users taken WHERE taken.id = old_keys.endpoint_id)
ORDER BY user_id, created_at;

UPDATE users SET id = moves.new_id FROM moves WHERE users.id = moves.old_id;

-- Memories name their owner in metadata, which no foreign key reaches. The
-- memory store makes its tables after migrations, so a new database has none.
DO $$
BEGIN
    IF to_regclass('memory_meta') IS NOT NULL THEN
        UPDATE memory_meta SET value = moves.new_id
        FROM moves
        WHERE memory_meta.key = 'user' AND memory_meta.value = moves.old_id;
    END IF;
END $$;

UPDATE users
SET name = old_keys.name, created_at = old_keys.created_at
FROM old_keys
WHERE users.id = old_keys.endpoint_id;

DROP TABLE old_keys;
