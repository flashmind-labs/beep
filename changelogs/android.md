# Android changelog

Earlier releases predate this changelog.

## [Unreleased]

### Added

- Android: search the Projects page by project name, folder or remote.

### Changed

- Android: a session's background commands fold into one line above the
  message field, such as "3 commands running"; tap it to open the Running
  tab, where each command has its Stop button. Queued messages fold into a
  line of their own, such as "4 queued"; tap it to edit or remove them.
- Android: the Home usage pill shows each harness's icon and the share of
  its most used limit, without bars.
- Android: the Projects page lists the most recently active projects first
  within each group, after pinned ones.
- Android: a running agent session, or one waiting on a question, stays
  followed after you leave it, and shows what it did at once when opened
  again. Returning to the app catches each one up once.

### Fixed

- Android: a Claude Code skill that runs in its own subagent shows as
  "Skill: <name> (subagent)" in the session log, so its steps no longer
  look like a plain skill load. Loading an ordinary skill stays a normal step.
- Android: a session whose turn ended while its subagents still run shows
  as working until they finish, not idle.
- Android: the message field and its Send and Stop buttons stay fully above
  the keyboard, and a log read to the end stays at the end.
- Android: the usage popover shows percentages, project and harness tokens,
  models and the total again.
- Android: the Projects page no longer runs off the side of the screen with
  long names, folders or group headings.
- Android: preserve composer drafts, focus and scroll during live updates.
- Android: reject stale approvals after their question or session changes.
- Android: notifications read, dismissed or opened on any desktop or phone
  are read here too, and opening the app no longer replays ones already read.
- Android: failed model lookups show Retry, preserve loaded models, and retry sooner.

### Performance

- Android: the app watches only the page's main area for new tooltips, and
  spinners stop turning while it is in the background.
- Android: update changed conversation rows independently during streaming.
- Android: returning to the app shows what changed while it was in the
  background instead of reloading every view, for up to a minute.
- Android: mesh links ping every 5 seconds instead of 3, so the radio
  sleeps more, and ride out a network drop of up to 30 seconds instead of
  reconnecting.
- Android: agent sessions followed in the background send nothing while
  the app is in the background and catch up once when it returns, and the
  palette and notifications check the mesh only when it changes.
