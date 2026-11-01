-- Tokens and API keys a user added once for Beep and their machines: a
-- GitHub token, a token per Forgejo or Gitea server, and one API key per
-- provider. `secret` is the value sealed with the server's credentials key
-- (nonce then ciphertext); the rest is what the apps show.
CREATE TABLE credentials (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    -- github, forgejo or api_key.
    kind TEXT NOT NULL,
    -- The API key's provider slug, such as openrouter; empty for git hosts.
    provider TEXT NOT NULL DEFAULT '',
    -- A Forgejo server's base URL; empty otherwise.
    host_url TEXT NOT NULL DEFAULT '',
    env_var TEXT NOT NULL DEFAULT '',
    to_machines BOOLEAN NOT NULL,
    last4 TEXT NOT NULL,
    -- The login the host said the token signs in as.
    account TEXT,
    secret BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, kind, provider, host_url)
);
