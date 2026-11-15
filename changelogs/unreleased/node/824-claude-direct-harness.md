### Added

- Node: Claude Code can run directly on the `claude` CLI instead of the npm
  ACP adapter, when its harness is set to Direct. Prompts, streamed text and
  thinking, tool calls, tool permission prompts, plan approval, Claude's
  questions (shown as forms when the app supports them), model and mode
  changes, stopping a turn, and resuming a session with its history all work.
  Claude Code also runs on a machine that has its CLI but neither its ACP
  adapter nor Node.js/npx, through the node's own Direct driver.
