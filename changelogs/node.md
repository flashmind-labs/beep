# Node changelog

Earlier releases predate this changelog.

## [Unreleased]

### Added

- Release pages include versioned release notes.
- The node metrics log counts started programs, file watch events, sync scans, topic changes, snapshot reads and state file writes.

### Fixed

- A session whose turn ended while its subagents still run reports them, so
  apps show it as working; node restarts and session moves wait for them.

### Performance

- The node uses less CPU while agents work: live views of sessions, processes, terminals and folders refresh at most a few times a second, and one session's view no longer lists every session.
- On macOS, idle agent sessions no longer run `ps` every second when no app shows their processes.
- Git branch, history and upstream views refresh only when Git refs, the index or settings change, not on every file save or Git object write.
- Builds and checkouts in synced projects no longer flood listings with one change signal per file.
