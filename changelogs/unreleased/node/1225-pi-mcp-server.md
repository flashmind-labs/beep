### Fixed

- Node: Pi sessions now register Beep's MCP server directly through Beep's Pi extension so Pi can call `start_agent`, `list_machines`, `list_projects`, and other Beep tools even though `pi-acp` drops `mcpServers`.
