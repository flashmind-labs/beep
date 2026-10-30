-- Standing permissions the user approved once for one routine: one acting
-- tool, held to the exact arguments in `constraints`, at most `per_run`
-- calls in each run of the routine. Deleting the routine ends its grants.
CREATE TABLE grants (
    id UUID PRIMARY KEY,
    user_id TEXT NOT NULL,
    routine_id TEXT NOT NULL,
    tool TEXT NOT NULL,
    constraints JSONB NOT NULL,
    per_run INTEGER NOT NULL,
    -- The card the user approved, as labelled fields, for the grant list.
    shown JSONB NOT NULL,
    -- The request_grant call the user answered, so an answer counts once.
    call_id TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, call_id),
    FOREIGN KEY (user_id, routine_id) REFERENCES crons (user_id, id)
        ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE INDEX grants_routine ON grants (user_id, routine_id);

-- Each call a grant let through, by the run it was made in.
CREATE TABLE grant_uses (
    grant_id UUID NOT NULL REFERENCES grants (id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    call_id TEXT NOT NULL,
    used_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (grant_id, run_id, call_id)
);
