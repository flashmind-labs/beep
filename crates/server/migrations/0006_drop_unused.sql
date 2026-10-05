-- Never written: routines report through runs and crons.last_run_at.
DROP TABLE cron_logs;
ALTER TABLE crons DROP COLUMN last_status, DROP COLUMN last_error;

-- Custom connections were saved but never used; MCP servers live in mcp_servers.
DROP TABLE mcp_connections;
