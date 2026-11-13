### Fixed

- Node: a folder that becomes a Git repository while it is being watched shows
  its Git state and later commits.
- Node: a project whose handover to Syncthing fails is retried instead of
  disappearing from sync.
- Node: after a project is handed to Syncthing, files Syncthing changed are
  not treated as local edits or deletions when Beep sync resumes.
- Node: with four devices attached, a fifth detaches the least recently used
  one, and shutting a device down by name or id detaches its viewer.
- Node: prompts sent while answering an agent's question count against the
  queue limit.
- Node: an alert is no longer skipped when many arrive at once.
- Node: a change in flsh credits is announced to connected apps.
- Node: worktrees are listed correctly when Git runs in another language.
- Node: the flsh update notice names version 0.2.9.
- Node: on Windows, the machine reports its real memory.
- Node: a host name longer than 64 bytes is shortened instead of shown as
  "Machine".
- Node: watching many folders no longer interrupts the connection to the
  desktop.
