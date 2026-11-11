# iOS changelog

Earlier releases predate this changelog.

## [Unreleased]

### Added

- iOS: swipe right from the left edge of the screen to go back, as the
  Back button does; the page follows your finger.
- iOS: how quickly screens open and respond to taps is kept in a
  metrics file in the app's data folder, by screen only and never with
  content.
- iOS: slow taps and stalls are noted in the app's log, by screen only and
  never with content, so slow screens can be found afterwards.
- iOS: Stop all, in a session's menu, ends its turn, subagents, background
  commands and the agents it started. It asks first while any of those
  run, and messages not yet delivered go back in the field.
- iOS: search the Projects page by project name, folder or remote.

### Changed

- iOS: the model picker shows flsh models by name, with their provider
  and full id under each, so a name offered by several providers can be told
  apart. Search matches the name, the id or the provider.
- iOS: a session's background commands fold into one line above the
  message field, such as "3 commands running"; tap it to open the Running
  tab, where each command has its Stop button. Queued messages fold into a
  line of their own, such as "4 queued"; tap it to edit or remove them.
- iOS: the Home usage pill shows provider balances in whole dollars,
  such as "$622 left"; the popover shows them to the cent.
- iOS: the Home usage pill shows each harness's icon and the share of its
  most used limit, without bars.
- iOS: the Projects page lists the most recently active projects first
  within each group, after pinned ones.
- iOS: a running agent session, or one waiting on a question, stays
  followed after you leave it, and shows what it did at once when opened
  again. Returning to the app catches each one up once.

### Fixed

- iOS: Stop all asks first when a session that is not open still has
  subagents running.
- iOS: Stop ends only the main agent's turn. Its subagents, background
  commands and the agents it started keep running, and the session shows
  as working until they finish.
- iOS: a Claude Code skill that runs in its own subagent shows as
  "Skill: <name> (subagent)" in the session log, so its steps no longer
  look like a plain skill load. Loading an ordinary skill stays a normal step.
- iOS: a session whose turn ended while its subagents still run shows
  as working until they finish, not idle.
- iOS: the message field and its Send and Stop buttons stay fully above
  the keyboard, and a log read to the end stays at the end.
- iOS: the usage popover shows percentages, project and harness tokens,
  models and the total again.
- iOS: the Projects page no longer runs off the side of the screen with
  long names, folders or group headings.
- iOS: preserve composer drafts, focus and scroll during live updates.
- iOS: reject stale approvals after their question or session changes.
- iOS: notifications read, dismissed or opened on any desktop or phone are
  read here too, and opening the app no longer replays ones already read.
- iOS: failed model lookups show Retry, preserve loaded models, and retry sooner.

### Performance

- iOS: a conversation opened after a restart or in a new window shows
  its last messages at once from disk, then catches up.
- iOS: a session shows its messages as soon as they arrive, without
  waiting for its modes, tasks and processes, and is read again only when
  following it fails.
- iOS: Home starts loading as the window opens.
- iOS: the app loads less than half the code at start; the terminal
  view loads when a terminal first shows.
- iOS: scrolling back in a conversation no longer lists every session
  on its machine for each page.
- iOS: the app watches only the page's main area for new tooltips, and
  spinners stop turning while it is in the background.
- iOS: update changed conversation rows independently during streaming.
- iOS: returning to the app shows what changed while it was in the
  background instead of reloading every view, for up to a minute.
- iOS: mesh links ping every 5 seconds instead of 3, so the radio sleeps
  more, and ride out a network drop of up to 30 seconds instead of
  reconnecting.
- iOS: agent sessions followed in the background send nothing while the
  app is in the background and catch up once when it returns, and the
  palette and notifications check the mesh only when it changes.
