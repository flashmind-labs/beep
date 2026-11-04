-- Bytes each node moved over iroh, as lifetime totals by key, such as
-- `alpn.beep-sync/1.tx`.
CREATE TABLE node_traffic (
    node TEXT NOT NULL REFERENCES nodes (endpoint_id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    -- The lifetime total.
    value BIGINT NOT NULL,
    -- The node process the last report came from, and its counter then: a
    -- report adds what it has over that, or all of it from a newer process.
    run_started_at BIGINT NOT NULL,
    run_value BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (node, key)
);
