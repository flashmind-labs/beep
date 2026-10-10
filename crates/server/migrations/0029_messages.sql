-- Every message the apps show becomes a row, and each user gets a log of
-- update ids over them. A client sends the last update id it holds and gets
-- everything after it. The model still reads `sessions.conversation` and
-- `sessions.context`; the apps read `messages`.
--
-- Session and group ids become the UUIDs the apps made them with, keyed in
-- `chats`. Group messages move here. Session messages come out of the
-- stored JSON, which only the server's Rust code reads: the server fills
-- them in for every session with `messages_backfilled` false before it
-- applies 0030, which drops the JSON.

CREATE TYPE chat_kind AS ENUM ('session', 'group');
CREATE TYPE message_role AS ENUM ('user', 'assistant', 'tool', 'scheduled', 'approval', 'member', 'beep');
CREATE TYPE update_op AS ENUM ('message', 'deleted', 'chat', 'chat_deleted');
CREATE TYPE reasoning_level AS ENUM ('off', 'low', 'medium', 'high');

-- Sessions and groups.
CREATE TABLE chats (
    id UUID PRIMARY KEY,
    kind chat_kind NOT NULL,
    -- A session's title, generated or the user's; a group's name.
    title TEXT,
    -- The user named it, so a generated title never replaces it.
    title_manual BOOLEAN NOT NULL DEFAULT FALSE,
    titled_at TIMESTAMPTZ,
    -- Creation order, which the session list follows.
    seq BIGINT GENERATED ALWAYS AS IDENTITY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- When its messages last changed.
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Who sees a chat, and their own settings for it. A session has one member,
-- its owner.
CREATE TABLE chat_members (
    chat_id UUID NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    -- What the others in a group see, picked when creating or joining. NULL
    -- in a session.
    name TEXT,
    creator BOOLEAN NOT NULL DEFAULT FALSE,
    pinned BOOLEAN NOT NULL DEFAULT FALSE,
    -- Beep may use this member's apps when someone else in the group asks.
    shares_apps BOOLEAN NOT NULL DEFAULT TRUE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, user_id)
);
CREATE INDEX chat_members_user ON chat_members (user_id);
-- Beep tells group members apart by name.
CREATE UNIQUE INDEX chat_members_name ON chat_members (chat_id, lower(name)) WHERE name IS NOT NULL;

CREATE TABLE messages (
    id BIGSERIAL PRIMARY KEY,
    chat_id UUID NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
    role message_role NOT NULL,
    -- The user message that opened this turn, its own id for a user message.
    -- NULL in groups, which have no turns.
    turn_id BIGINT REFERENCES messages (id) ON DELETE CASCADE,
    -- The person who wrote it. NULL for Beep and its steps.
    author_id TEXT REFERENCES users (id) ON UPDATE CASCADE ON DELETE SET NULL,
    -- Kept so the messages of someone who left a group still show their name.
    author_name TEXT,
    -- The text. A routine's label for 'scheduled', empty for tool steps and
    -- approvals.
    content TEXT NOT NULL DEFAULT '',
    sent_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- The client's id for a message it sent, so a retry posts once.
    request_id UUID,
    UNIQUE (chat_id, request_id)
);
CREATE INDEX messages_chat ON messages (chat_id, id);
CREATE INDEX messages_turn ON messages (turn_id);
-- Searching chats for any words.
CREATE EXTENSION IF NOT EXISTS pg_trgm WITH SCHEMA public;
CREATE INDEX messages_search ON messages USING gin (content gin_trgm_ops);

-- What a turn ran with, keyed by the message that opened it: the user's
-- message, or the scheduled prompt of a routine.
CREATE TABLE turns (
    message_id BIGINT PRIMARY KEY REFERENCES messages (id) ON DELETE CASCADE,
    -- The model that answered. Empty when not recorded.
    model TEXT NOT NULL DEFAULT '',
    -- What Jev picked the thinking at, when it picked the model.
    auto_thinking reasoning_level,
    -- The model's reasoning over every step, oldest first.
    reasoning TEXT NOT NULL DEFAULT '',
    -- How likely the turn holds something worth remembering, once scored.
    memory_score DOUBLE PRECISION
);

-- role 'tool'
CREATE TABLE tool_calls (
    message_id BIGINT PRIMARY KEY REFERENCES messages (id) ON DELETE CASCADE,
    call_id TEXT NOT NULL,
    name TEXT NOT NULL,
    -- Cut to 16 KiB.
    output TEXT NOT NULL,
    success BOOLEAN NOT NULL
);
CREATE INDEX tool_calls_call ON tool_calls (call_id);

-- role 'approval': a run stopped to ask before a tool call.
CREATE TABLE approvals (
    message_id BIGINT PRIMARY KEY REFERENCES messages (id) ON DELETE CASCADE,
    query_id TEXT NOT NULL,
    app TEXT NOT NULL,
    action TEXT NOT NULL
);
CREATE TABLE approval_fields (
    message_id BIGINT NOT NULL REFERENCES approvals (message_id) ON DELETE CASCADE,
    position INT NOT NULL,
    label TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (message_id, position)
);

-- Each user's last update id. Taking the next one locks this row, so a
-- user's updates commit in id order and "everything after N" skips none.
-- Tombstones older than `pruned_below` are gone, so a client behind it
-- starts over.
CREATE TABLE sync_counters (
    user_id TEXT PRIMARY KEY REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    last BIGINT NOT NULL DEFAULT 0,
    pruned_below BIGINT NOT NULL DEFAULT 0
);

-- Each user's log: one row per message they can see and one per chat. A
-- change moves the row to a new update id, so the log never outgrows the
-- messages. `message_id` has no foreign key: a 'deleted' row keeps the id
-- of the message that went.
CREATE TABLE updates (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    update_id BIGINT NOT NULL,
    chat_id UUID NOT NULL,
    -- NULL for a chat-level update.
    message_id BIGINT,
    op update_op NOT NULL,
    -- When it last moved, so old tombstones can go.
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, update_id)
);
CREATE UNIQUE INDEX updates_message ON updates (user_id, message_id) WHERE message_id IS NOT NULL;
CREATE UNIQUE INDEX updates_chat ON updates (user_id, chat_id) WHERE message_id IS NULL;

-- Old ids to new. A session id is only unique per user, so a second user's
-- session with the same id gets a fresh one, as does any id that is not a
-- UUID.
CREATE TEMPORARY TABLE chat_ids (
    kind chat_kind NOT NULL,
    -- The session's owner; '' for groups.
    owner TEXT NOT NULL,
    old_id TEXT NOT NULL,
    new_id UUID NOT NULL UNIQUE,
    PRIMARY KEY (kind, owner, old_id)
) ON COMMIT DROP;

INSERT INTO chat_ids (kind, owner, old_id, new_id)
SELECT 'session', user_id, id,
    CASE WHEN first AND id ~* '^[0-9a-f]{8}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{12}$'
        THEN id::uuid ELSE gen_random_uuid() END
FROM (
    SELECT user_id, id, row_number() OVER (PARTITION BY lower(replace(id, '-', '')) ORDER BY seq) = 1 AS first
    FROM sessions
) s;

INSERT INTO chat_ids (kind, owner, old_id, new_id)
SELECT 'group', '', id,
    CASE WHEN id ~* '^[0-9a-f]{8}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{12}$'
            AND NOT EXISTS (SELECT 1 FROM chat_ids taken WHERE taken.new_id = id::uuid)
        THEN id::uuid ELSE gen_random_uuid() END
FROM groups;

-- Sessions, in their old order.
INSERT INTO chats (id, kind, title, title_manual, titled_at, created_at, updated_at)
SELECT m.new_id, 'session', t.title, COALESCE(t.manual, FALSE), t.titled_at,
    COALESCE(s.updated_at, now()), COALESCE(s.updated_at, now())
FROM sessions s
JOIN chat_ids m ON m.kind = 'session' AND m.owner = s.user_id AND m.old_id = s.id
LEFT JOIN session_titles t ON t.user_id = s.user_id AND t.session_id = s.id
ORDER BY s.seq;

INSERT INTO chat_members (chat_id, user_id, pinned)
SELECT m.new_id, s.user_id, s.pinned
FROM sessions s
JOIN chat_ids m ON m.kind = 'session' AND m.owner = s.user_id AND m.old_id = s.id;

INSERT INTO chats (id, kind, title, created_at, updated_at)
SELECT m.new_id, 'group', g.name, g.created_at, g.updated_at
FROM groups g
JOIN chat_ids m ON m.kind = 'group' AND m.old_id = g.id
ORDER BY g.created_at;

INSERT INTO chat_members (chat_id, user_id, name, creator, pinned, shares_apps, joined_at)
SELECT m.new_id, gm.user_id, gm.name, gm.creator, gm.pinned, gm.shares_apps, gm.joined_at
FROM group_members gm
JOIN chat_ids m ON m.kind = 'group' AND m.old_id = gm.group_id;

-- Group messages move here in their old order. `legacy_seq` carries the
-- old number until their pictures point at the new ids.
ALTER TABLE messages ADD COLUMN legacy_seq BIGINT;
INSERT INTO messages (chat_id, role, author_id, author_name, content, sent_at, request_id, legacy_seq)
SELECT m.new_id,
    CASE WHEN gm.author_id IS NULL THEN 'beep'::message_role ELSE 'member'::message_role END,
    a.id, gm.author_name, gm.content, gm.sent_at,
    CASE WHEN gm.request_id ~* '^[0-9a-f]{8}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{12}$'
        THEN gm.request_id::uuid END,
    gm.seq
FROM group_messages gm
JOIN chat_ids m ON m.kind = 'group' AND m.old_id = gm.group_id
-- An author whose account is gone keeps only the name.
LEFT JOIN users a ON a.id = gm.author_id
ORDER BY gm.seq;

-- What stays of a group besides its chat: the invite.
CREATE TABLE group_invites (
    chat_id UUID PRIMARY KEY REFERENCES chats (id) ON DELETE CASCADE,
    -- The token in the invite link. Resetting the link replaces it.
    invite TEXT NOT NULL UNIQUE
);
INSERT INTO group_invites (chat_id, invite)
SELECT m.new_id, g.invite
FROM groups g
JOIN chat_ids m ON m.kind = 'group' AND m.old_id = g.id;

-- Routines: the chat they reply in stays without a foreign key, since a run
-- makes the chat again when the user deleted it. A group reminder goes with
-- its group.
ALTER TABLE crons ADD COLUMN chat_id UUID, ADD COLUMN group_chat_id UUID REFERENCES chats (id) ON DELETE CASCADE;
UPDATE crons c SET chat_id = COALESCE(
    (SELECT new_id FROM chat_ids m WHERE m.kind = 'session' AND m.owner = c.user_id AND m.old_id = c.session_id),
    CASE WHEN c.session_id ~* '^[0-9a-f]{8}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{12}$'
        THEN c.session_id::uuid END)
WHERE c.session_id IS NOT NULL;
UPDATE crons c SET group_chat_id = m.new_id
FROM chat_ids m WHERE m.kind = 'group' AND m.old_id = c.group_id;
DROP INDEX crons_group;
ALTER TABLE crons DROP COLUMN session_id, DROP COLUMN group_id;
ALTER TABLE crons RENAME COLUMN group_chat_id TO group_id;
CREATE INDEX crons_group ON crons (group_id) WHERE group_id IS NOT NULL;

-- Runs now belong to a chat, which the server makes when the first run
-- starts. A run whose session was never saved or was deleted goes.
DELETE FROM runs r WHERE NOT EXISTS (
    SELECT 1 FROM chat_ids m WHERE m.kind = 'session' AND m.owner = r.user_id AND m.old_id = r.session_id);
ALTER TABLE runs ADD COLUMN chat_id UUID REFERENCES chats (id) ON DELETE CASCADE;
UPDATE runs r SET chat_id = m.new_id
FROM chat_ids m WHERE m.kind = 'session' AND m.owner = r.user_id AND m.old_id = r.session_id;
ALTER TABLE runs ALTER COLUMN chat_id SET NOT NULL;
DROP INDEX one_active_run;
DROP INDEX idx_runs_user_session;
ALTER TABLE runs DROP COLUMN session_id;
CREATE UNIQUE INDEX one_active_run ON runs (chat_id) WHERE status = 'running';
CREATE INDEX runs_chat ON runs (chat_id, seq);

-- Shares go with their session.
DELETE FROM shares s WHERE NOT EXISTS (
    SELECT 1 FROM chat_ids m WHERE m.kind = 'session' AND m.owner = s.user_id AND m.old_id = s.session_id);
ALTER TABLE shares ADD COLUMN chat_id UUID REFERENCES chats (id) ON DELETE CASCADE;
UPDATE shares s SET chat_id = m.new_id
FROM chat_ids m WHERE m.kind = 'session' AND m.owner = s.user_id AND m.old_id = s.session_id;
ALTER TABLE shares ALTER COLUMN chat_id SET NOT NULL;
ALTER TABLE shares DROP COLUMN session_id;
ALTER TABLE shares ADD UNIQUE (chat_id);

-- Files keep their chat without a foreign key: deleting a chat deletes its
-- files' blobs first, and a file whose chat is gone expires like any other.
-- A picture points at its message. Session pictures still name their turn
-- by `turn_at` until the backfill sets `message_id`.
ALTER TABLE files
    ADD COLUMN chat_id UUID,
    ADD COLUMN message_id BIGINT REFERENCES messages (id) ON DELETE SET NULL;
UPDATE files f SET chat_id = m.new_id
FROM chat_ids m
WHERE f.group_seq IS NULL AND m.kind = 'session' AND m.owner = f.user_id AND m.old_id = f.session_id;
UPDATE files f SET chat_id = m.new_id,
    message_id = (SELECT id FROM messages WHERE chat_id = m.new_id AND legacy_seq = f.group_seq)
FROM chat_ids m
WHERE f.group_seq IS NOT NULL AND m.kind = 'group' AND m.old_id = f.session_id;
ALTER TABLE messages DROP COLUMN legacy_seq;
DROP INDEX files_session;
ALTER TABLE files DROP COLUMN session_id, DROP COLUMN group_seq;
CREATE INDEX files_chat ON files (chat_id);
CREATE INDEX files_message ON files (message_id) WHERE message_id IS NOT NULL;

-- Read once by the backfill, which folds them into turns and tool_calls;
-- 0030 drops them. They take the new ids now so it can find them.
DELETE FROM tool_failures t WHERE NOT EXISTS (
    SELECT 1 FROM chat_ids m WHERE m.kind = 'session' AND m.owner = t.user_id AND m.old_id = t.session_id);
UPDATE tool_failures t SET session_id = m.new_id::text
FROM chat_ids m WHERE m.kind = 'session' AND m.owner = t.user_id AND m.old_id = t.session_id;
DELETE FROM memory_scores t WHERE NOT EXISTS (
    SELECT 1 FROM chat_ids m WHERE m.kind = 'session' AND m.owner = t.user_id AND m.old_id = t.session_id);
UPDATE memory_scores t SET session_id = m.new_id::text
FROM chat_ids m WHERE m.kind = 'session' AND m.owner = t.user_id AND m.old_id = t.session_id;
DELETE FROM turn_models t WHERE NOT EXISTS (
    SELECT 1 FROM chat_ids m WHERE m.kind = 'session' AND m.owner = t.user_id AND m.old_id = t.session_id);
UPDATE turn_models t SET session_id = m.new_id::text
FROM chat_ids m WHERE m.kind = 'session' AND m.owner = t.user_id AND m.old_id = t.session_id;

-- What stays of a session: what the model reads, and until 0030 the
-- transcript the backfill reads. Its owner, pin and order are in chats and
-- chat_members now.
ALTER TABLE sessions ADD COLUMN chat_id UUID;
UPDATE sessions s SET chat_id = m.new_id
FROM chat_ids m WHERE m.kind = 'session' AND m.owner = s.user_id AND m.old_id = s.id;
ALTER TABLE sessions DROP CONSTRAINT sessions_pkey;
ALTER TABLE sessions DROP COLUMN user_id, DROP COLUMN id, DROP COLUMN seq, DROP COLUMN pinned, DROP COLUMN updated_at;
ALTER TABLE sessions ALTER COLUMN chat_id SET NOT NULL;
ALTER TABLE sessions ADD PRIMARY KEY (chat_id);
ALTER TABLE sessions ADD FOREIGN KEY (chat_id) REFERENCES chats (id) ON DELETE CASCADE;
ALTER TABLE sessions ADD COLUMN messages_backfilled BOOLEAN NOT NULL DEFAULT FALSE;
-- New sessions write their messages as they go.
ALTER TABLE sessions ALTER COLUMN messages_backfilled SET DEFAULT TRUE;

-- Every user's log so far: each group member gets the group's messages and
-- the chat, each owner their sessions. The backfill adds session messages.
INSERT INTO updates (user_id, update_id, chat_id, message_id, op)
SELECT user_id, row_number() OVER (PARTITION BY user_id ORDER BY sort, message_id), chat_id, message_id, op
FROM (
    SELECT cm.user_id, cm.chat_id, NULL::bigint AS message_id, 'chat'::update_op AS op, 0 AS sort
    FROM chat_members cm
    UNION ALL
    SELECT cm.user_id, msg.chat_id, msg.id, 'message', 1
    FROM chat_members cm
    JOIN messages msg ON msg.chat_id = cm.chat_id
) log;
INSERT INTO sync_counters (user_id, last)
SELECT id, COALESCE((SELECT max(update_id) FROM updates WHERE user_id = users.id), 0)
FROM users;

DROP TABLE session_titles;
DROP TABLE group_messages;
DROP TABLE group_members;
DROP TABLE groups;
