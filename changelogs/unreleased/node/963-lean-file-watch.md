### Performance

- Node: on Linux, the file watch skips build output and caches
  (target, node_modules, .venv, __pycache__, .next and similar) and
  linked worktrees under .worktrees. Opening the Files tab on a large
  checkout starts watching much faster and uses far fewer inotify
  watches, so the desktop no longer reports that the session
  subscription did not answer in time.

### Fixed

- Node: changes made by a session working in a linked worktree inside a
  project's .worktrees folder now reach views that follow all session
  folders. Before, those changes were dropped.
