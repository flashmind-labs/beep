# Android changelog

Earlier releases predate this changelog.

## [Unreleased]

## [0.2.4] - 2026-11-11

### Added

- Android: a message sent to a session whose machine cannot be reached waits on
  this device, in the session's queue marked "waiting for connection", and
  goes in the order typed once the machine is back, even after the app
  restarts. Edit or remove it like other queued messages; one the machine
  refuses stays with the reason.
- Android: how quickly screens open and respond to taps is kept in a
  metrics file in the app's data folder, by screen only and never with
  content.
- Android: slow taps and stalls are noted in the app's log, by screen only
  and never with content, so slow screens can be found afterwards.
- Android: Stop all, in a session's menu, ends its turn, subagents, background
  commands and the agents it started. It asks first while any of those
  run, and messages not yet delivered go back in the field.
- Android: search the Projects page by project name, folder or remote.
- Android: fold a group on the Projects page with a tap on its name. It
  stays folded on this phone; a search still shows its matches.

### Changed

- Android: the model picker shows flsh models by name, with their provider
  and full id under each, so a name offered by several providers can be told
  apart. Search matches the name, the id or the provider.
- Android: a session's background commands fold into one line above the
  message field, such as "3 commands running"; tap it to open the Running
  tab, where each command has its Stop button. Queued messages fold into a
  line of their own, such as "4 queued"; tap it to edit or remove them.
- Android: the Home usage pill shows provider balances in whole dollars,
  such as "$622 left"; the popover shows them to the cent.
- Android: the Home usage pill shows each harness's icon and the share of
  its most used limit, without bars.
- Android: the Projects page lists the most recently active projects first
  within each group, after pinned ones.
- Android: a running agent session, or one waiting on a question, stays
  followed after you leave it, and shows what it did at once when opened
  again. Returning to the app catches each one up once.
- Android: each row on the Coding agents page shows its harness icon, and
  New sits in the page header instead of a second title under it.
- Android: a session's message field and its buttons sit in one rounded
  box, without a border around the field itself.

### Fixed

- Android: buttons are compact (Import, Reply, Review and the others) instead of full touch-target height; rows and menus keep their larger targets.
- Android: an open session no longer stays on "Lost the connection...
  Reconnecting..." until the app restarts. When a machine stops taking new
  reads while still connected, as after switching between mobile data and
  Wi-Fi or coming back from the background, the app connects to it again
  and the session catches up on its own.
- Android: the Agents list shows sessions that start, end or change while it is open, without leaving the page and coming back.

- Android: Stop all asks first when a session that is not open still has
  subagents running.
- Android: Stop ends only the main agent's turn. Its subagents, background
  commands and the agents it started keep running, and the session shows
  as working until they finish.
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
- Android: the Projects and Coding agents pages no longer scroll
  sideways or stay shifted left after swiping back, and long session
  titles end in an ellipsis.
- Android: a session's branch chip no longer starts with a stray dot.

### Performance

- Android: a conversation opened after a restart or in a new window shows
  its last messages at once from disk, then catches up.
- Android: a session shows its messages as soon as they arrive, without
  waiting for its modes, tasks and processes, and is read again only when
  following it fails.
- Android: Home starts loading as the window opens.
- Android: the app loads less than half the code at start; the terminal
  view loads when a terminal first shows.
- Android: scrolling back in a conversation no longer lists every session
  on its machine for each page.
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

