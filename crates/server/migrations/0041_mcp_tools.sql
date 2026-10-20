-- The tools of an added server the user turned on, by their names on the
-- server. NULL until the user picks: then the first 25 usable ones are on.
ALTER TABLE mcp_servers ADD COLUMN tools TEXT[];
