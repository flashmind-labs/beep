### Added

- Node: a session can start in a throwaway copy of a checkout: a detached
  worktree with its uncommitted changes and untracked files copied in. The copy
  is removed when the session ends, other than with the node, or is deleted.
  Desktop challenges use it.
- Node: installs a `challenge` skill that the desktop's Challenge button asks a
  fresh agent to follow: it checks the report's claims in its copy and reports
  each broken claim in a shape the desktop places on the report.
