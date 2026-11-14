### Changed

- Desktop: the move menu moves a session to the project folder on the
  other machine. It no longer offers a worktree there, and it no longer
  stops because a machine cannot make worktrees or ignores them in
  Syncthing. Committed branches still reach that machine through Git.
  Uncommitted work in a worktree stays where it is.
