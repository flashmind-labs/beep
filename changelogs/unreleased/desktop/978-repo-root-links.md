### Fixed

- Desktop: a file link in a reply from a session working in a subfolder
  of a repository now opens in the file pane when the file is in a
  worktree at the repository's top, such as `.worktrees/fix/...`.
  Before, the pane looked only in the session's folder and failed.
