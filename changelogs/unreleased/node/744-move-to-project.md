### Changed

- Node: a session moved to this machine resumes in the project folder,
  and the node no longer makes a worktree for it. Committed branches
  arrive through Git. Uncommitted work in a worktree stays on the
  machine it was on.
