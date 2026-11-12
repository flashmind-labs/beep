### Added

- Desktop: a file path an agent writes in inline code, such as
  `src/main.rs:42`, opens in the file pane, and a line number in a file link
  scrolls to and marks that line.

### Changed

- Desktop: a relative file link opens the file from the worktree the agent
  works in, falling back to the session's folder.
