-- What Beep did in the user's apps, like sending an email, so starters
-- never suggest it again. Kept a few weeks.
CREATE TABLE actions (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    done_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    description TEXT NOT NULL
);
CREATE INDEX idx_actions_user_done ON actions (user_id, done_at DESC);
