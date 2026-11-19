### Added

- Node: Codex can run on a Direct driver. beep-node now drives
  `codex app-server` itself instead of the npm ACP adapter, so Codex
  sessions no longer need the adapter or Node.js. Modes, model, reasoning
  effort, fast mode, approvals, questions, steering and history work as
  before. Codex inside WSL keeps using its adapter.
- Node: on the Direct driver, a Codex command's output shows in its tool
  entry.

### Changed

- Node: Claude Code and Codex on the Direct driver, and coding agents
  inside WSL, now run inside the node itself. beep-node no longer starts
  a second copy of itself for each session.
