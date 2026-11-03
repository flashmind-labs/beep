-- A user's flsh settings (OpenRouter routing and compaction), the same on
-- every machine: `beep_proto::FlshSettings` as JSON. Nodes write them to
-- flsh's config.toml before they start flsh.
CREATE TABLE flsh_settings (
    user_id TEXT PRIMARY KEY REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    settings JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
