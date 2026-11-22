### Added

- beep-node: an assistant chat can start a thread as a custom agent, such as
  a `.claude/agents/researcher.md` file or a Codex, OpenCode, pi or flsh agent
  file, from the chat's folder or the user's agents on that machine. The
  thread follows that agent's instructions, runs on its harness unless the
  chat names another, and keeps the agent across restarts and moves; an
  unknown name is refused with the names that exist.
