-- MCP servers the user added by URL. Fastmail's row leaves `url` empty: its
-- URL is fixed in code. `sign_in` is set once the server asked for OAuth.
ALTER TABLE mcp_servers
    ADD COLUMN url TEXT,
    ADD COLUMN label TEXT,
    ADD COLUMN sign_in BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT now();
