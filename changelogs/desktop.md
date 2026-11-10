# Desktop changelog

Desktop changes are collected under Unreleased, then archived by version and
date. Releases before 0.6.0 predate this changelog.

Other apps have independent versions and changelogs:
[node](node.md), [Android](android.md),
[iOS](ios.md), and [server](server.md).

## [Unreleased]

### Added

- Desktop: search the Projects page by project name, folder or remote; press
  / or ⌘F to start typing.
- macOS: check for updates hourly and show release notes in Sparkle's standard
  native update dialog.
- Release notes are kept for each version and included in desktop update feeds.

### Changed

- Desktop: a double tap of Option opens the bar on the sessions waiting for
  you: questions first, then finished turns you have not read, then running
  and recent sessions. Return opens one with its composer, Esc comes back, ⌘N
  starts a new agent. With nothing recent the bar opens on the new agent form.
- Desktop: a session's background commands and queued messages take at
  most about a third of the window and scroll inside it.
- Desktop: the usage pill shows each harness's icon and the share of its
  most used limit, without bars. The popover hides empty sections and keeps
  its note on dollars to periods that show dollars.
- Desktop: the Projects page lists the most recently active projects first
  within each group, after pinned ones.
- Desktop: a running agent session, or one waiting on a question, stays
  followed after you leave it or hide the window, and shows what it did at
  once when opened again. Idle sessions are let go after five minutes.

### Fixed

- Desktop: a session whose turn ended while its subagents still run shows
  as working until they finish, not idle.
- Desktop: in a narrow window the Projects page no longer runs off the
  side with long names, folders or group headings.
- Desktop: preserve panel scroll, drafts and selections during live updates.
- Desktop: reject stale approvals after their question or session changes.
- Desktop: retain terminal controls when returning to a cached session.
- Desktop: a notification read, dismissed or opened on any desktop or phone
  is read on all of them, and one read elsewhere no longer pops up again
  after the Mac wakes.
- Desktop: view-only terminals on the Terminals page refresh their output again,
  and keep focus, selection and scroll position between reads.

### Performance

- Desktop: update changed chat rows independently and virtualize long project
  session lists.
- Desktop: share concurrent machine and chat list reads.
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
- Desktop: running agents, chats and research show a still dot instead of a
  spinner, and "Thinking" pulses a few times and then holds, so long runs
  no longer redraw the window every frame. Pages also watch fewer changes.
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
