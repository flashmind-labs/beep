### Changed

- beep-node: agent sessions always run in the project folder, which is
  the repository's main checkout. A session asked to work in a worktree
  runs in the project and is told to work in that worktree. A session
  may use several worktrees and branches. Once its work merges, the
  session is told to remove every worktree it created.

### Fixed

- beep-node: a session whose worktree was removed resumes in its project
  folder after beep-node restarts, instead of failing to resume.
