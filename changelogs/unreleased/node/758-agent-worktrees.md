### Added

- Node: a session remembers the worktrees it created, both the one Beep
  made when it started and the ones the agent added with
  `git worktree add`. The list survives node restarts and goes with the
  session when it moves to another machine.
