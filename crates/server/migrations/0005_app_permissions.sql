-- How freely Beep acts in each app: 'ask', 'allow' or 'read'. `allowed` holds
-- the tools that run without asking under 'ask'. No row means 'ask'.
CREATE TABLE app_permissions (
    user_id TEXT NOT NULL REFERENCES users (id) ON UPDATE CASCADE ON DELETE CASCADE,
    app TEXT NOT NULL,
    level TEXT NOT NULL,
    allowed TEXT[] NOT NULL DEFAULT '{}',
    PRIMARY KEY (user_id, app)
);
