-- Recent emails Jev judged to need the user's attention, kept until the
-- evening digest reports or drops them. `text` is the email as the digest
-- shows it.
CREATE TABLE attention_items (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    mailbox TEXT NOT NULL,
    message_id TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL,
    text TEXT NOT NULL,
    PRIMARY KEY (user_id, mailbox, message_id)
);

-- What a routine runs: 'prompt' wakes the agent with its prompt, 'digest'
-- gathers the evening digest first.
ALTER TABLE crons ADD COLUMN kind TEXT NOT NULL DEFAULT 'prompt';

-- Set once the digest routine was made, so one the user deleted stays deleted.
ALTER TABLE users ADD COLUMN digest_offered BOOLEAN NOT NULL DEFAULT FALSE;
