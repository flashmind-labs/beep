-- When Beep last looked through each of a user's machines for checkouts of
-- their imported projects (see `crate::imports::checks`).
CREATE TABLE checkout_checks (
    user_id TEXT NOT NULL,
    -- The node's key, as `nodes.endpoint_id`.
    endpoint_id TEXT NOT NULL,
    checked_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, endpoint_id)
);
