### Fixed

- Node: a terminal that exits stays in the list for 30 seconds, so every
  connected desktop and phone sees that it ended. Before, only the first
  one to look saw it, and it vanished for the others.
- Node: a background command that was still starting when the desktop app
  quit is now stopped with the others instead of running on.
