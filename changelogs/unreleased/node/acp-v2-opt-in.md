### Added

- Node: ACP v2 support is back but off by default; set BEEP_ACP_VERSION=2 in
  an agent's environment to try it (it falls back to v1 when the agent only
  speaks v1). Agents with v2 now accept the Beep MCP server instead of failing
  to start.
