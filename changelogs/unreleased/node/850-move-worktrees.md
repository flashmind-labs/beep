### Changed

- Node: moving a session no longer waits for every file of the project.
  The target waits until its checkout is at the same commit as the
  source's.
- Node: the worktrees a session created move with it, including their
  branch, commits and uncommitted changes. Other worktrees stay where
  they are. The source keeps its copy of a moved worktree, detached from
  the branch.
