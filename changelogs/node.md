# Node changelog

Earlier releases predate this changelog.

## [Unreleased]

### Added

- Release pages include versioned release notes.

### Fixed

- Node: a session's log keeps Beep's whole agent brief as context instead of cutting off its last sentences.
- Node: a project waiting on Git keeps its warning instead of flickering between scanning, synced and failed on every scan.
- Node: the worktree menu leaves out worktrees of other machines listed in a synced `.git` folder, and sessions keep their folder when an agent runs a command on another machine.

