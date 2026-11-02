-- Reviews of a project's branch before a merge: read-only coding-agent
-- sessions each read one shard of the diff, and a fresh verifier session
-- checks what they found. The sessions run on a node; these rows say which
-- ones, and keep what they reported.
CREATE TABLE branch_reviews (
    id UUID PRIMARY KEY,
    user_id TEXT NOT NULL,
    -- org/repo.
    project TEXT NOT NULL,
    branch TEXT NOT NULL,
    base TEXT NOT NULL,
    -- The node whose checkout the reviewers read, as `nodes.endpoint_id`.
    endpoint_id TEXT NOT NULL,
    folder TEXT NOT NULL,
    head TEXT NOT NULL,
    merge_base TEXT NOT NULL,
    head_at TIMESTAMPTZ NOT NULL,
    changed_lines INTEGER NOT NULL,
    files INTEGER NOT NULL,
    -- NULL when git could not say; else the files that conflict.
    conflicts TEXT[],
    pr_number BIGINT,
    pr_url TEXT,
    -- none, pending, passing or failing.
    pr_checks TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX branch_reviews_project ON branch_reviews (user_id, project, started_at DESC);

CREATE TABLE branch_review_shards (
    review_id UUID NOT NULL REFERENCES branch_reviews (id) ON DELETE CASCADE,
    shard INTEGER NOT NULL,
    area TEXT NOT NULL,
    paths TEXT[] NOT NULL,
    files INTEGER NOT NULL,
    lines INTEGER NOT NULL,
    agent TEXT NOT NULL,
    -- The reviewer's and the verifier's session ids on the node.
    session TEXT NOT NULL,
    verifier TEXT,
    -- reading, verifying, done or failed.
    state TEXT NOT NULL,
    error TEXT,
    PRIMARY KEY (review_id, shard)
);

CREATE TABLE branch_review_findings (
    review_id UUID NOT NULL REFERENCES branch_reviews (id) ON DELETE CASCADE,
    id INTEGER NOT NULL,
    shard INTEGER NOT NULL,
    severity TEXT NOT NULL,
    title TEXT NOT NULL,
    file TEXT NOT NULL,
    line INTEGER,
    why TEXT NOT NULL,
    -- NULL while the verifier works; verified, dropped or unchecked.
    verdict TEXT,
    verdict_why TEXT,
    dismissed BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (review_id, id)
);
