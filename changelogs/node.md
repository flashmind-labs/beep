# Node changelog

Earlier releases predate this changelog.

## [Unreleased]

### Added

- Release pages include versioned release notes.
- The node metrics log counts started programs, file watch events, sync scans, topic changes, snapshot reads and state file writes.

### Changed

- Node: flsh models get readable names, such as "DeepSeek V4.1 Flash 1017"
  for `openrouter:deepseek/deepseek-v4.1-flash-1017`, with the provider
  reported apart from the name.

### Fixed

- Node: Stop ends only the main agent's turn. Its subagents, background
  commands and the agents it started keep running, and the session shows
  as working until they finish.
- Node: a steering message reaches the agent at its next step while
  background commands and subagents run, instead of waiting for them.
- Node: a Claude Code skill that runs in its own subagent shows as
  "Skill: <name> (subagent)" in the session log, so its steps no longer
  look like a plain skill load. Loading an ordinary skill stays a normal step.
- A session whose turn ended while its subagents still run reports them, so
  apps show it as working; node restarts and session moves wait for them.
- The "turn finished" notification waits until the session's subagents end
  too, or until they send nothing for 30 minutes. Questions still notify at once.
- Node: a session's log keeps Beep's whole agent brief as context instead of cutting off its last sentences.
- Node: a project waiting on Git keeps its warning instead of flickering between scanning, synced and failed on every scan.
- Node: the worktree menu leaves out worktrees of other machines listed in a synced `.git` folder, and sessions keep their folder when an agent runs a command on another machine.
- Node: a session whose background subagent finished, or whose turn was stopped mid-subagent, stops showing as working and sends its "turn finished" notification right away.
- Node: more session views can stay open across desktop windows and phones at once (128 instead of 32); idle views drop their cached transcript when memory runs short.
- Node: closing a session stops dev servers its commands left running, even when a wrapper started them and exited at once.
- Node: Git branch and history views refresh for a folder that became a repository, or whose Git lookup failed once, after its next file change.

### Performance

- The node uses less CPU while agents work: live views of sessions, processes, terminals and folders refresh at most a few times a second, and one session's view no longer lists every session.
- On macOS, idle agent sessions no longer run `ps` every second when no app shows their processes.
- Git branch, history and upstream views refresh only when Git refs, the index or settings change, not on every file save or Git object write.
- Builds and checkouts in synced projects no longer flood listings with one change signal per file.
- Streaming agent sessions open in an app cost the node less CPU: each update encodes only new or changed transcript entries.
- Node: an idle machine wakes far less often. Mesh links ping every 5
  seconds instead of 3, the health check runs every 15 seconds instead of 5,
  the timer lag probe runs with the minute's metrics instead of every second,
  and machine reports, the CI watch, skill checks, session copies and mesh
  settings run only when something changed or much less often.
- Node: a mesh link rides out a network drop of up to 30 seconds instead of
  reconnecting after 10, and a machine that stays offline is dialed about once
  a minute instead of every 12 seconds. One that comes back is reached at once.
