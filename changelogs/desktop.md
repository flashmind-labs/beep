# Desktop changelog

Desktop changes are collected under Unreleased, then archived by version and
date. Releases before 0.6.0 predate this changelog.

Other apps have independent versions and changelogs:
[node](node.md), [Android](android.md),
[iOS](ios.md), and [server](server.md).

## [Unreleased]

## [0.8.6] - 2026-11-12

### Changed

- Desktop: diagrams in chats use the chat's colors instead of a white card, show at full size, and say what kind of diagram they are. Copy image, PNG and SVG moved into an Export menu next to Source and Copy. Sequence diagrams show each participant once, at the top.
- Desktop: the project menu (⋯) groups its sync settings under "Sync on all machines" as checkmarks instead of "On/Off (all machines)" labels. Choose ignored folders shows only while ignored folders sync, the Syncthing switch reads "Use Beep sync instead of Syncthing" (or back), and Remove project comes last after a line. All projects left the menu; Esc or Back goes there.

### Fixed

- Desktop: a connection to a machine that stops answering is dropped and dialed again after about 15 seconds instead of 30.
- Desktop: after the Mac wakes from sleep, machines reconnect within seconds instead of staying on connecting for up to a minute, and messages queued meanwhile go out once they do.
- Desktop: a right-click menu whose lines arrive after it opens stays inside the window instead of running off its edge.
- Desktop: ⌘M (Ctrl+M on Windows) in a project's task field opens the model picker, as its hint says.
- Desktop: the folder list in Sync gitignored takes the sheet's full width instead of a small box.
- Desktop: the sidebar says which machine it is connecting to ("Connecting to build-box and 1 more...", all of them on hover), and its connecting dot is a small ring again instead of a wide pill.
- Desktop: with more than 500 sessions in the lists, the app no longer asks every machine where each session works over and over, which kept a core busy and wrote thousands of log lines a minute.

### Performance

- Desktop: a session's place, a file, a worktree list or a folder listing asked for once no longer stays live on its machine, and live views no one reads again are dropped after half a minute. Machines stop reading them again after every change, which saves CPU on the machines and on the desktop.
- Desktop: the places of many sessions started outside Beep are asked of each machine in one request.

## [0.8.5] - 2026-11-11

### Added

- Desktop: switching pages fades the new page in. Reduce motion turns it off.

### Fixed

- Desktop: the Files tab no longer jumps to the top or flashes empty when you
  come back to a session; it shows the tree where you left it. A session
  working in a worktree stays on that worktree instead of briefly showing the
  session's own folder.

## [0.8.4] - 2026-11-11

### Fixed

- Desktop: a machine that stops taking new reads while still connected is
  connected to again, instead of leaving sessions, files and lists on
  "Lost the connection" until the app restarts.
- Desktop: the Agents list shows sessions that start, end or change while it is open, without leaving the page and coming back.

### Performance

- Desktop: opening Add a project no longer leaves Beep looking for checkouts
  again after every session update. On a Mac with active sessions it ran git
  thousands of times a minute until the app restarted. The list of checkouts
  is read once when the sheet asks, and a checkout's details are read again
  only after a commit, branch switch or remote change in it.

## [0.8.3] - 2026-11-11

### Added

- Desktop: a message sent to a session whose machine cannot be reached waits on
  this device, in the session's queue marked "waiting for connection", and
  goes in the order typed once the machine is back, even after the app
  restarts. Edit or remove it like other queued messages; one the machine
  refuses stays with the reason.
- Desktop: a Claude Code, Codex, pi, flsh or OpenCode session running in a
  terminal has a Continue here button. After you confirm, Beep waits for a
  running turn to end, closes the session in its terminal and continues it
  here. The wait can be cancelled.
- Desktop: fold a group on the Projects page by clicking its name. It
  stays folded on this computer; a search still shows its matches.

### Changed

- Desktop: each row in the Coding agents list shows its harness icon.

### Fixed

- Projects: a long group name stays on one line and ends in "..." instead of wrapping over several lines.
- Desktop: on opening or waking, sessions, Home and machines shown from what
  the app kept now say they are still connecting. A machine not reached yet
  has a hollow dot and reads "connecting", its sessions are dimmed, and an
  open conversation says "Connecting to" its machine until the first live
  update arrives. Before, the kept view looked live until it suddenly
  changed.
- Desktop: a long activity line on a running session no longer widens
  the Coding agents list past the window.

### Performance

- Desktop (macOS): Beep checks far less often whether a coding session
  running in a terminal is still open, which cuts its background CPU use and
  the `lsof` and `ps` processes it starts. A session that starts, ends or is
  resumed still shows within seconds.

## [0.8.2] - 2026-11-11

### Fixed

- Desktop: Codex installed with its own installer shows its updates and
  updates with `codex update`.
- Desktop: the Files tab of a session no longer flickers while the agent
  works; an update redraws only the files that changed and keeps the scroll
  and selection.

### Performance

- Desktop: the Files tab draws only the rows in view, so folders with
  thousands of files scroll and update without stalling.

## [0.8.1] - 2026-11-11

### Performance

- Desktop: while a Claude Code, Codex or flsh session runs outside Beep,
  the lists of such sessions are read again at most every 2 seconds instead
  of 4 times a second each, so the machine scans its agents' files far less.
- Desktop: the Files tab draws only the rows in view, so folders with
  thousands of files scroll and update without stalling.

## [0.8.0] - 2026-11-11

### Added

- Desktop: records how quickly pages open and respond to clicks and keys,
  by page only and never with content, in the app's local metrics, so slow
  pages can be found afterwards.
- Desktop: Stop all, in a session's menu, ends its turn, subagents, background
  commands and the agents it started. It asks first while any of those
  run, and messages not yet delivered go back in the field.
- Desktop: search the Projects page by project name, folder or remote; press
  / or ⌘F to start typing.
- macOS: check for updates hourly and show release notes in Sparkle's standard
  native update dialog.
- Release notes are kept for each version and included in desktop update feeds.

### Changed

- Desktop: every window and the bar draw their pages in one consistent way,
  so lists, drafts, focus, selections and scroll stay where they were while
  live updates arrive, and pages open without flicker.
- Desktop: the model picker shows flsh models by name, with their provider
  and full id under each, so a name offered by several providers can be told
  apart. Search matches the name, the id or the provider.
- Desktop: a double tap of Option opens the bar on the sessions waiting for
  you: questions first, then finished turns you have not read, then running
  and recent sessions. Return opens one with its composer, Esc comes back, ⌘N
  starts a new agent. With nothing recent the bar opens on the new agent form.
- Desktop: a session's background commands and queued messages take at
  most about a third of the window and scroll inside it.
- Desktop: a session's background commands show as one line above the
  message field, such as "3 commands running", instead of a row each. Click
  it to open the Running tab, which lists each command with its Stop button.
- Desktop: the usage pill shows each harness's icon and the share of its
  most used limit, without bars. The popover hides empty sections and keeps
  its note on dollars to periods that show dollars.
- Desktop: the usage pill shows provider balances in whole dollars, such as
  "$622 left"; the popover shows them to the cent.
- Desktop: the Projects page lists the most recently active projects first
  within each group, after pinned ones.
- Desktop: a running agent session, or one waiting on a question, stays
  followed after you leave it or hide the window, and shows what it did at
  once when opened again. Idle sessions are let go after five minutes.

### Fixed

- Desktop: project counts in the sidebar line up with the counts above them;
  the pin shows in the count's place on hover.

- Desktop: right after the app starts, session rows ask each machine once
  for where their sessions work and try again a few seconds later while a
  machine is still starting, instead of failing and waiting half a minute.
- Desktop: a session on a busy machine opens instead of reporting a lost
  connection when its log takes more than a few seconds to read.
- Desktop: Stop all asks first when a session that is not open still has
  subagents running.
- Desktop: Stop ends only the main agent's turn. Its subagents, background
  commands and the agents it started keep running, and the session shows
  as working until they finish.
- Desktop: the model search in a project's new-agent composer keeps what
  you type, its caret, focus and highlighted model while the project or the
  model list is still loading.
- Desktop: a Claude Code skill that runs in its own subagent shows as
  "Skill: <name> (subagent)" in the session log, so its steps no longer
  look like a plain skill load. Loading an ordinary skill stays a normal step.
- Desktop: a session whose turn ended while its subagents still run shows
  as working until they finish, not idle.
- Desktop: in a narrow window the Projects page no longer runs off the
  side with long names, folders or group headings.
- Desktop: panels keep their scroll position, drafts and selections while
  live updates arrive.
- Desktop: an approval no longer goes to a question or session that changed
  since it was shown.
- Desktop: a session's terminal controls stay when you return to it.
- Desktop: a notification read, dismissed or opened on any desktop or phone
  is read on all of them, and one read elsewhere no longer pops up again
  after the Mac wakes.
- Desktop: view-only terminals on the Terminals page refresh their output again,
  and keep focus, selection and scroll position between reads.
- Desktop: typing in the worktree search no longer loses the caret or scrambles the text when the list refreshes.
- Desktop: a session's Files and Changes no longer switch to a folder on another machine after the agent runs a command there.
- Desktop: failed model lookups show Retry, preserve loaded models, and retry sooner.
- Desktop: Stop all on a session of a node from 0.5.0 or earlier stops its
  turn, instead of breaking every live view of that machine.
- Desktop: an agent's question reaches the desktop at once after its node
  restarts, instead of up to a minute later.
- Desktop: after a sleep or a Wi-Fi drop, machines reconnect at once.

### Performance

- Desktop: while agents work, a change on one machine updates only that
  machine's sessions instead of listing every machine again, which before
  could repeat dozens of times a second and fill the log with hundreds of
  megabytes a day. Home redraws for session changes at most once a second.

- Desktop: a diff of thousands of files opens faster and scrolls without
  dropped frames, a project page opens with less work, and the model picker
  opens and filters quickly with hundreds of models.
- Desktop: switching between recent sessions puts their messages back as
  they were instead of drawing them again, and a streaming reply redraws
  only its newest turn.
- Desktop: sessions that stream in the background save their last messages
  to disk at most once a minute and when a turn ends, instead of every few
  seconds, with less work in the window.
- Desktop: a conversation opened after a restart or in a new window shows
  its last messages at once from disk, then catches up.
- Desktop: a session shows its messages as soon as they arrive, without
  waiting for its modes, tasks and processes, and is read again only when
  following it fails.
- Desktop: Home starts loading as the window opens, and on first launch
  the welcome screen no longer holds Home back for up to 3 seconds.
- Desktop: windows load less than half the code at start; the terminal
  view loads when a terminal first shows.
- Desktop: scrolling back in a conversation no longer lists every session
  on its machine for each page.
- Desktop: Home draws from what the last run kept at about the moment the
  window opens, instead of showing "Connecting" until every machine answers.
  Machines, projects and accounts no longer wait for each other, the main
  window loads before the hidden bar, and on a Mac the login shell is no
  longer read before the window opens, except on the first launch.
- Desktop: spinners stop turning while the window is hidden, sync progress
  and review and Actions status follow at most once a second, and turning on
  "Use this Mac" no longer reloads all of Home twice. Chat diagrams and the
  picture viewer no longer watch every change to the page.
- Desktop: a chat redraws only the rows that changed, and long project
  session lists draw only the rows on screen.
- Desktop: windows that ask for the machine or chat list at the same time
  share one read.
- Desktop: use less CPU in the background while projects sync: a sync change
  reads only the projects it touched, and machine clock checks are shared.
- Desktop: Home, File sync and the saved window views do less work while agents
  run and files sync: one session's change no longer lists every machine,
  unchanged views are not sent again, and transfer progress redraws at most
  once a second.
- Desktop: a hidden window stops streaming terminals, polling research and
  reloading the bar's chat list. Terminals catch up on the output from
  meanwhile when shown again.
- Desktop: a minimized window, or on macOS one that is fully covered or on
  another Space, pauses its background updates like a closed one.
- Desktop: "Thinking" and "Working" sweep at 15 frames a second instead of
  every frame, and spinners pause while the window is hidden. Pages also watch fewer changes.
- Desktop: while agents run, Home, the sidebar, the usage pill and project
  pages update at most once a second (project sessions every two seconds)
  and skip redraws when nothing changed. Approvals and questions still show
  at once.
- Desktop: Home, Terminals, Sync, project pages, Coding agents and the agent
  list open at once from what the window already holds. They read again only
  after something they show changed, instead of on every visit.
- Desktop: a session's Changes and Files tabs show what they had at once when
  you return to them, for the last four sessions and folders, and read the
  diff or folders again only when something in the folder changed.
- Desktop: large diffs and reviews open and scroll smoothly. Only the files
  and lines near the viewport are drawn, in the diff and in its file list, so
  a branch with hundreds of changed files no longer slows the whole window.
- Desktop: hiding and showing a window no longer reloads every view. Live
  updates pause while it is hidden and, if it shows again within five
  minutes, only what changed meanwhile is sent. Sending a prompt or another
  action reloads only the views it affects.
- Desktop: mesh links ping every 5 seconds instead of 3 and ride out a
  network drop of up to 30 seconds. A machine that stays away is tried about
  twice a minute instead of every 12 seconds. Agent alerts and CI runs are followed
  only from machines that are online, and a machine that keeps refusing is
  tried at most once a minute instead of every 2 seconds.
- Desktop: an idle or hidden window wakes far less often. Agent sessions
  followed in the background send nothing while the window is hidden and
  catch up once when it shows. The command palette, the bell and machine
  follows react to mesh changes instead of checking every few seconds, and
  closed terminal views stop at once.
- Desktop: a session followed in a hidden window keeps only the entries it
  shows instead of its whole run, and saving a followed session's page takes
  less work on each update.

## [0.7.0] - 2026-11-09

### Added

- A setting to pause project sync across all machines.
- Coding agents can discover projects and select a project when starting an agent.

### Fixed

- Model choices made before resuming a session and configured default models are
  reflected correctly in the session controls.
- Live updates no longer interrupt touch scrolling in conversations.
- Editor launch failures are shown instead of leaving the action unexplained.
- Missing Codex subagent conversations are recovered from their saved sessions.
- Windows background desktop commands no longer open console windows.

### Performance

- Project sync scans changed paths instead of rescanning whole projects for
  every filesystem change.

## [0.6.10] - 2026-11-09

### Fixed

- Project and machine icons stay visible when the sidebar is collapsed.
- Diff files appear in the same order as the changed-files tree.

## [0.6.9] - 2026-11-09

### Added

- Move coding agent sessions between projects.
- Copy quoted replies with a dedicated button.
- Collapse the navigation sidebar while keeping its icons available.

### Changed

- Single-folder chains take less space in the changed-files tree.
- Chat searches can match terms across different messages in the same conversation.

### Fixed

- Pasted text is preserved in the project composer.
- Session controls show the model reported by the session.
- Bundled diagrams can load in conversation panes.
- Missing image files have an explanation in the conversation.
- Windows background availability checks no longer open console windows.

## [0.6.8] - 2026-11-09

### Changed

- Project setup controls and discovered-project lists use a more compact layout.

### Fixed

- Conversations stay at the bottom when the composer changes height.
- Diff gutters keep their background colors when scrolling.
- Focusing a conversation no longer changes its header border color.

## [0.6.7] - 2026-11-09

### Changed

- Wide reply tables scroll horizontally instead of breaking words.

### Fixed

- Session panes restore correctly and terminals keep running when changing panes.
- Model selections resolve display names to the model identifiers agents accept,
  and retain the selected model for the session.
- Bundled diagrams can run inside sandboxed conversation previews.

## [0.6.6] - 2026-11-09

### Changed

- Project machines, sync settings, and terminals are grouped together.
- Coding agent settings distinguish shared preferences from settings specific
  to a machine.

### Fixed

- Session streams and model choices are restored when reopening a session.
- Saved instructions are supplied to coding agent sessions.
- HTML previews can load scripts, fonts, and relative assets.
- Git views and file context menus stay open during background refreshes.
- Removal dialogs can be dismissed while work is pending.
- Actions keep existing runs visible when a machine cannot be reached.
- Resumed session state is applied before sending attached images.

## [0.6.5] - 2026-11-09

### Added

- Temporary warnings explain session moves that have not finished yet.
- Expand the surrounding context in a diff.

### Fixed

- Session and worktree lists stay stable during background refreshes.
- The changed-files tree keeps the current diff file in view.
- Moved sessions no longer leave duplicate rows in project session lists.
- Diagrams render again after reopening a conversation pane.
- Reopened session panes restore their previous views.

## [0.6.4] - 2026-11-09

### Added

- Open HTML previews from file menus.

### Fixed

- Default model suggestions show the model names.
- Long messages and nested Markdown lists wrap within their columns.

## [0.6.3] - 2026-11-09

### Added

- Notification read state is shared between apps through the mesh.

### Fixed

- Git panels stay visible during project refreshes.
- The Running pane refreshes a session's child processes.
- Steering messages sent from another app appear when reading the session.
- Chat search floats above the conversation instead of moving its content.
- The hide-keyboard control stays hidden outside the phone layout.

## [0.6.2] - 2026-11-08

### Added

- Windows and Linux desktop installers.
- Choose a default model for each coding agent harness.
- Preview HTML files in the desktop file pane.
- Copy selected coding agent output as Markdown.
- Interrupted coding agent turns can continue after a restart.

### Fixed

- Steering messages remain visible above the composer.
- Continuation summaries no longer replace restored session titles.
- Diff line numbers are excluded from copied text.
- Notifications return to the project they were opened from.
- The command palette keeps its selection when scrolling under a stationary
  pointer.

## [0.6.1] - 2026-11-08

### Added

- Search conversations with a compact search box and regular expressions.
- Navigate notifications with the keyboard.
- See the latest command while a coding agent turn is running.
- Open remote file links in their default local app.

### Changed

- Conversation compaction is shown more clearly in session logs.

### Fixed

- Steering messages appear immediately after sending.
- Project refreshes preserve session clicks.
- Long session lists continue loading older entries.
- Finished notifications clear when their session is read through a live stream.

### Performance

- Diff scrolling and progress animations avoid unnecessary layout work.

## [0.6.0] - 2026-11-08

### Added

- Windows desktop support and direct linking between desktop apps.
- Hermes coding agent support.
- Rename coding agent sessions by double-clicking their titles.
- Resize the diff file list and browse collapsible Git history and worktrees.
- Attach files across coding agent inputs and keep messages in a persistent
  project stash.
- Open subagents in the normal conversation view and see running reviewers
  under the sessions they review.
- Render Mermaid diagrams with image export and clipboard actions.
- Sync selected Git-ignored project folders and recover local edits during
  Git sync when recovery is enabled.
- Open local app actions from file references in agent replies.
- Open notifications with a keyboard shortcut.

### Changed

- Git branches sync independently of the branch checked out on each machine.
- Command rows expand to show the actual command and its output.

### Fixed

- Hidden conversation views restore their scroll positions when shown again.
- Delivered steering messages no longer remain in the composer.
- Opened build notifications are dismissed.
- Reviewers receive the project's reviewer instructions and can continue the
  same review session on subsequent reviews.

### Security

- External sites cannot navigate over desktop app windows.

### Performance

- Live mesh subscriptions refresh sessions and project views without repeated
  full reads.
- Hidden terminal views release their renderers until shown again.
