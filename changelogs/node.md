# Node changelog

Earlier releases predate this changelog.

## [0.6.5] - 2026-11-12

### Changed

- Node: agents name machines without apostrophes or symbols, so
  "Alex's Mac Studio" is `alexs-mac-studio` instead of `alex's-mac-studio`.

### Fixed

- Node: a connection to a machine that stops answering is dropped and dialed again after about 15 seconds instead of 30.
- Node: on a laptop that sleeps, the node reconnects to the other machines within seconds of waking instead of up to a minute later.

### Performance

- Node: looking up one session started outside Beep reads only that session's file instead of every Claude Code transcript on the machine, and several are answered in one request.
- Node: lookups that older desktops keep live wait longer between reads when nothing changed, so an older desktop no longer keeps a node busy.
- Node: a request that ends well within a second no longer writes four lines to the log; only slow, failed or dropped requests are logged.

## [0.6.4] - 2026-11-11

### Performance

- Node: listing the checkouts sessions worked in reads each checkout with git
  only after a commit, branch switch or remote change in it, skips git for
  folders outside any checkout, and shares one listing between requests that
  arrive together. A desktop up to 0.8.3 that keeps this list open gets it at
  most once a minute instead of after every session update.

## [0.6.3] - 2026-11-11

### Added

- Node: a desktop can close a session running in a terminal so it continues
  in Beep. The node ends only the process it found running that session,
  with SIGTERM on Linux and macOS; on Windows the process is ended at once.

### Performance

- Node: checks of which process holds a Codex or flsh session's file are
  shared by every list and open session view, kept until the file is written
  or its process exits, and run for many files at once. Finding Claude Code
  sessions reuses the node's process list and reads each process's folder
  once. This cuts the `lsof` and `ps` processes started on macOS.

## [0.6.2] - 2026-11-11

### Fixed

- Node: Codex installed with its own installer shows its updates and
  updates with `codex update`.

## [0.6.1] - 2026-11-11

### Performance

- Node: while a Claude Code, Codex or flsh session runs outside Beep,
  the lists of such sessions are read again at most every 2 seconds instead
  of 4 times a second each, so the machine scans its agents' files far less.

## [0.6.0] - 2026-11-11

### Added

- Release pages include versioned release notes.
- The node metrics log counts started programs, file watch events, sync scans, topic changes, snapshot reads and state file writes.

### Changed

- Node: flsh models get readable names, such as "DeepSeek V4.1 Flash 1017"
  for `openrouter:deepseek/deepseek-v4.1-flash-1017`, with the provider
  reported apart from the name.

### Fixed

- Node: listing one agent's models no longer waits for another agent's
  slow listing, so Hermes and flsh answer the start form promptly.
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
- Node: a node started with its own `--dir` keeps its session presence file in that folder, so it no longer replaces or removes the installed node's.
- Node: an action from a newer desktop or phone that this node cannot read
  yet gets an error back, instead of dropping every live view on that link.
- Node: Stop all also stops the background commands and subagents of agents
  the session started on other machines, not only their turns.
- Node: a Claude Code transcript replaced by a new file is always read again
  in full, even when its size and time match the old one.
- Node: after a sleep or a Wi-Fi drop, mesh links to other machines are
  dialed again at once.

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
  reconnecting after 10, and a machine that stays offline is dialed about twice
  a minute instead of every 12 seconds. One that comes back is reached at once.
- Node: conversations open faster. A newly opened session sends its newest
  page first, a Claude Code transcript parses only the lines added since the
  last read, its subagents' files are read again only when they change, and a
  followed session looks up its file once and its processes only when they
  may have changed.
- Node: following a Claude Code session started outside Beep takes far less
  CPU: about 1% of a core instead of 17 to 50% for a session writing 20 lines
  a second. Each update parses only the newly written lines
  for the session's title and last prompt, reads only the entries the app
  does not already hold, reads the process list at most every 10 seconds
  when no process runs the session, and waits 200 ms to batch writes. Chats
  Beep runs still update at once.
- Node: following a Claude Code session lists its project folder only when a
  file comes or goes there, not on every update.

