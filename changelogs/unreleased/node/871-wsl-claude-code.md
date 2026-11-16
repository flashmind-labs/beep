### Added

- Node: on a Windows machine without Claude Code installed on Windows,
  the node now runs the Claude Code installed in a WSL distribution as
  that machine's Claude Code. Sessions work on the project's Windows
  folder, seen from WSL under `/mnt/`. The machine's Claude Code sign-in
  is passed into WSL, and Beep tools work there. The WSL distribution no
  longer needs to be added as a separate machine.
