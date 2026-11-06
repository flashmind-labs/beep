-- What the server told each user's devices, for the apps' events stream: a
-- stream reads from here, so it resumes after the last notice it read, and
-- any server instance serves it. Rows go after a week.
CREATE TABLE app_notices (
    id BIGSERIAL PRIMARY KEY,
    user_id TEXT NOT NULL,
    -- The notice, as msgpack with field names.
    notice BYTEA NOT NULL,
    -- What the user's computers get instead, when it differs.
    desktop BYTEA,
    -- Which devices get it: everyone, desktops, or the one key in `device`.
    reach TEXT NOT NULL,
    device TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX app_notices_user ON app_notices (user_id, id);

-- The devices that took a notice off their stream. A phone that did not is
-- reached through FCM or APNs instead.
CREATE TABLE app_notice_acks (
    notice_id BIGINT NOT NULL REFERENCES app_notices (id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    PRIMARY KEY (notice_id, key)
);

-- Every instance listens on `beep_events`: `n <id> <user>` for a notice,
-- `u <last> <user>` for an update log that moved, `k 0 <key>` to end a key's
-- streams. The update log tells it from here, on commit.
CREATE FUNCTION notify_sync_counter() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('beep_events', 'u ' || NEW.last || ' ' || NEW.user_id);
    RETURN NEW;
END
$$ LANGUAGE plpgsql;

CREATE TRIGGER sync_counters_notify
    AFTER INSERT OR UPDATE ON sync_counters
    FOR EACH ROW EXECUTE FUNCTION notify_sync_counter();
