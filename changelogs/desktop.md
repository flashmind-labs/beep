# Desktop changelog

Pending changes are files in [unreleased/desktop](unreleased/desktop), collected
here by version and date at release. Releases before 0.6.0 predate this changelog.

Other apps have independent versions and changelogs:
[node](node.md), [Android](android.md),
[iOS](ios.md), and [server](server.md).

## [0.8.33] - 2026-11-14

### Added

- Desktop: sign in to Claude on individual online machines from Agent accounts.
- Desktop: a review agent that hands in its review sends a notification.
  Opening it, or the running review from the diff, shows the reviewer's
  conversation with the Changes tab beside it.

### Changed

- Desktop: a working agent, a running command, a CI run and research show
  a turning spinner again instead of a still dot. It pauses while the
  window is hidden.
- Desktop: the first-run screens have a new look, with a painted sunrise
  above the choices. "Link another desktop" is now "Join my other
  computer" and says what it does.
- Desktop: in Set up this computer, each row has its own Install or Sign
  in button, so one agent can be signed in without setting up the rest.
- Desktop: the start screen shows a painted sunrise behind the headline
  instead of the dotted one; the sun rises once after the app opens and
  then stays still.
- Desktop: the sidebar has a painted dawn haze down it, strongest at the
  top, instead of the dots in its corner.
- Desktop: an empty Home and an empty Projects page show the sunrise with
  the next step on a card. The sun rises once and stays risen from screen
  to screen.
- Desktop: Add a project shows its three ways as tiles, and each found
  folder as a plain row whose Import button appears when pointed at.
- Desktop: each project card has a small painted swatch in its colour.

### Fixed

- Desktop: moving a coding agent session to another machine no longer
  refuses with "the session works outside the project's checkout" when the
  session started in the project's checkout and later worked in one of its
  worktrees.
- Desktop: Claude sign-in detection on macOS now follows Claude's auth status instead of treating leftover Keychain entries as a sign-in.

### Performance

- Desktop: while a project syncs, its sync details are read at most
  every ten seconds instead of on every change, which uses less battery.
- Desktop: the app log no longer fills with network warnings after
  waking from sleep, and writes one line per request to another machine
  instead of three.

## [0.8.32] - 2026-11-14

### Fixed

- Desktop: model settings hide unavailable context limits and their explanation while keeping supported compaction controls.
- Desktop: a picture opened from the attachments window fills the app
  instead of showing as small as the window.
- Desktop: an agent's message is no longer cut in two when a background
  subagent calls a tool while the message is being written (needs the updated
  node).

## [0.8.31] - 2026-11-13

### Changed

- Desktop: the app has a new look. Labels, tabs, buttons and numbers use a
  monospaced face on a dark background. Menus, dialogs, the palette, the
  message field and Settings sit on frosted glass. The start screen keeps
  its dot art. Settings opens as a glass sheet.
- Desktop: pane tabs and the Device toolbar are smaller.
- Desktop: running commands and queued messages sit in a compact tray
  attached to the top of the message field.
- Desktop: attached files show on their own line above the message, with a
  file count.
- Desktop: context use shows as a small meter beside the percentage.
- Desktop: project cards show what runs, when it last ran and where, in two
  lines.
- Desktop: a project's Machines section shows one compact card per machine.
  Folder and Remove appear on hover. The Terminals row shows only when a
  terminal runs.
- Desktop: a message's send time shows in its corner on hover instead of on
  its own line. A sent picture no longer repeats its file name.
- Desktop: the Sync page and its sidebar link are gone. Each project's
  Machines section now shows its sync state and any conflicts. Conflict
  cards on Home and on a project's board, and conflict notifications, open
  that section, and a project with conflicts shows their count on its
  sidebar row.

### Fixed

- Desktop: a message of several lines stays inside its field.
- Desktop: a failed upload's message goes away after a few seconds and when
  the next upload starts.
- Desktop: when a resumed session refuses a model switch, the reason shows
  instead of "[object Object]".

## [0.8.30] - 2026-11-13

### Fixed

- Desktop: Claude Code and Codex chats start again. The ACP v2 support added
  in the previous release made them fail with "data did not match any
  variant of untagged enum McpServer", so it was rolled back to ACP v1.

### Performance

- Desktop: reduced background work for macOS notifications and agent session updates.

## [0.8.29] - 2026-11-13

### Added

- Desktop: prefer ACP v2 for agents, fall back to v1 when an agent selects it, and allow `BEEP_ACP_VERSION=1` in the harness environment to select v1.
- Desktop: open uploaded attachments from messages, and play video from the original local copy on the uploading device while that copy is available.

### Fixed

- Desktop: avoid duplicate ACP v2 subagent steps, keep idle sessions inactive after failed compaction, and retain model, effort and mode controls after incomplete configuration replies.
- Desktop: switching conversations no longer stops uploads or sends files to the wrong chat.
- Desktop: a submitted message sends to the conversation it was written in, even after its attachments finish uploading.
- Desktop: conversations already at the bottom stay there when sending images.
- Desktop: interruption notices appear as status instead of becoming part of the next user prompt.

## [0.8.28] - 2026-11-13

### Added

- Desktop: prompts, agent responses, and tools show discreet timestamps when available.

### Changed

- Desktop: child conversations stay in the parent agent's Subagents section and no longer appear in conversation lists or completion notifications.

### Fixed

- Desktop: submitted prompts remain visible and greyed out while attachments upload, then send automatically.
- Desktop: image prompts sent while a resumed agent starts no longer need to be retried.
- Desktop: a node connection that goes silent is noticed and opened again
  instead of waiting forever.
- Desktop: messages kept for an agent on another machine are sent after the
  app restarts.
- Desktop: on Linux, logs are written under the XDG state folder instead of
  ~/Library/Logs/Beep.
- Desktop: one terminal that cannot be cleaned up no longer empties the whole
  terminal list.
- Desktop: a refused pause, resolve or start-over of file sync shows its
  error.
- Desktop: Send in a review works again after a failed send, and "Try again"
  in a problem report works more than once.
- Desktop: Preview in the Files menu opens the preview instead of the source.
- Desktop: a failed command shows the same exit code in every row.
- Desktop: Home shows Gemini CLI, pi and flsh by name.
- Desktop: a session in a nested project is counted under the same project
  everywhere.
- Desktop: one machine on an older version no longer stops agent modes from
  loading for other machines.
- Desktop: terminal text with both strikethrough and overline shows both.
- Desktop: saving two agent settings at once keeps both, and a newly added
  token with push access is used at once.
- Desktop: a refused clone from GitLab, Bitbucket or another Git host is
  reported as not found or no access.

## [0.8.27] - 2026-11-13

### Fixed

- Desktop: App labels and controls no longer select text accidentally; chat messages and editable fields remain selectable.
- Desktop: Clicking the emulator no longer shows a blue focus border.

## [0.8.26] - 2026-11-13

### Fixed

- Desktop: Agent chat sends automatically after attachments and image previews finish uploading; failed uploads keep the draft for retry.
- Desktop: View and remove attachments in a compact composer window with image previews; long sent file lists fold.
- Desktop: automatic session titles are shorter and no longer show internal tags.
- Desktop: attachments and media can be uploaded, opened and downloaded without the previous per-file size limits.

## [0.8.25] - 2026-11-13

### Added

- Desktop: Settings > Skills lists skill libraries, packs of agent skills kept
  in a Git repository. Add one from its Git URL, Update every machine to the
  newest commit, or remove it.
- Desktop: install a whole library or single skills on every machine, or for
  one project from the project's Skills… menu. Keep out of Git is on by
  default for project skills.
- Desktop: Find existing libraries adopts skills already in the agents' global
  skills folders that came from a Git repository. Name clashes and failed
  clones show under the library.
- Desktop: rename projects from the project menu, with an option to rename their hosted repository while keeping local folder paths.
- Desktop: select repository copies on GitHub and Forgejo separately when renaming a project, and retry failed hosts while keeping successful renames.
- Desktop: subagent lists show the full model name from the chat model picker when it is known.
- Desktop: sort Home projects by recent activity or last opened, with a separate preference from Projects.

### Changed

- Desktop: the log records how long each stage of a launch took, from the
  process starting to Home being drawn, in lines that start with `startup:`.
- Desktop: a smaller project rename dialog shows repository settings only when selected.
- Desktop: every checkbox uses the same style, including issue picking, ignored-folder
  sync, confirm dialogs, Show ignored in Files and Include binary path.
- Desktop: Home and Projects use a styled sort menu.

### Fixed

- Desktop: blocked conversations show the provider reason alongside any partial reply, including in saved history.
- Desktop: uploaded attachments in project folders no longer appear as files made by the agent.
- Desktop: closed sessions appear in history as soon as the machine reports they ended.
- Desktop: Claude API errors in conversations appear in yellow again.
- Desktop: flsh installs and updates on Windows x86_64 using the published Windows executable and checksum.

### Performance

- Desktop: Beep opens about a third faster. The window shows before the rest
  of the app is set up, and the metrics clock no longer holds the launch for
  200 ms on Apple silicon.
- Desktop: Home appears before the window's scripts have loaded, showing what it
  last showed.

## [0.8.24] - 2026-11-13

### Added

- Desktop: when the agent boots, shows or drives an emulator or simulator,
  the session's Device tab opens on that device, in any project. The
  conversation names the device and machine, such as "Booted iPhone 17 on
  mac-box".
- Desktop: choose Default, Recent activity or Last opened when sorting projects. The choice is remembered.

### Changed

- Desktop: the agent pane's Device tab shows only for sessions in an Android
  or iOS app, and starts on an emulator or simulator of that platform, on
  another machine when the session's has none. A platform no machine has a
  device for is named in one line.

### Fixed

- Desktop: adding a project to a machine shows the bytes copied, the rate,
  the time left and a progress bar, instead of "0 of 11 files" until the
  copy ends.
- Desktop: the code block Copy button stays at the right edge while scrolling horizontally.
- Desktop: list rows in projects and Home, such as pull requests, agents and
  the project sections, use their normal title size again instead of large
  bold text.
- Desktop: a steer delivered while you were on another session no longer
  stays above the message field when you come back.

## [0.8.23] - 2026-11-13

### Added

- Desktop: Backspace at the start of a coding agent's message field removes the last
  attached picture or file.

### Changed

- Desktop: a picture attached to a coding agent message uploads in full, like other files,
  and the agent gets a small preview with the message. It is no longer shrunk.

## [0.8.22] - 2026-11-13

### Added

- Desktop: in a diff, where arrows showed more unchanged lines a button beside
  them hides those lines again.

### Changed

- Desktop: a file link that points at a missing file names the path and machine
  it tried, instead of saying the file is gone or the machine is offline.
- Desktop: the agent draft for reconciling Git sync is laid out as Markdown sections instead of raw JSON.

### Fixed

- Desktop: a project folder that was replaced on a machine, for example by
  cloning it again, says so on the Sync page and offers Start over with this
  folder, instead of staying stopped with no way out.

## [0.8.21] - 2026-11-13

### Added

- Desktop: Settings, Credentials has API keys for OpenRouter, Meshy, ElevenLabs
  and other providers. Every desktop shares them, and each machine gives them
  to flsh as env vars for its image, video, 3D and speech tools.
- Desktop: Appearance settings can hide prompt marks in conversation scrollbars or hide the scrollbar entirely.
- Desktop: a Device tab in the agent pane shows an Android emulator or iOS
  simulator from any machine, live; tap, drag, type and press its buttons
  there, and boot or shut it down from the tab.
- Desktop: in a diff, arrows beside a change's header show ten more of the
  unchanged lines before it, from either side.

### Changed

- Desktop: a diff no longer has a row counting the unchanged lines between
  two changes.

### Fixed

- Desktop: session message headers show the source project and session title instead of raw session tags or IDs.

## [0.8.20] - 2026-11-13

### Fixed

- Desktop: problem reports from builds without a configured server go to the
  report receiver instead of the product website.
- Desktop: the Review button on a Home board card opens its menu instead of
  opening the pull request.

## [0.8.19] - 2026-11-12

### Added

- Desktop: Start on the first-run screen checks this computer for Git,
  Node.js, Claude Code and Codex, installs what is missing with one Set up
  button, then opens each agent's sign-in. Any item can be skipped.

### Changed

- Desktop: Settings shows Agent accounts, where you set the Claude Code token
  and the Codex API key your machines use.
- Desktop: Home's board names each project above its row, with the project's
  dot, so the cards get the full width and no longer repeat the project's
  name.
- Desktop: a link to an HTML file in an agent's reply opens the page rendered
  in the file pane; Source shows its code.

## [0.8.18] - 2026-11-12

### Added

- Desktop: audio files (MP3, M4A, AAC, WAV, FLAC, Ogg, Opus) named in an agent's reply or opened from Files play in the side pane.
- Desktop: a two-finger swipe sideways on the trackpad goes back or forward.
  The page follows the fingers and comes back if they stop short.

### Changed

- Desktop: Home's board has a row for each project, with the projects that
  wait on you first. A project's name opens it, a cell shows about two cards
  and scrolls to the rest, and a project's sync conflicts share one card.
- Desktop: the Review button of a pull request on the board is a menu: Open
  shows it on its host, and Review with agent starts an agent's review.

## [0.8.17] - 2026-11-12

### Added

- Desktop: a file path an agent writes in inline code, such as
  `src/main.rs:42`, opens in the file pane, and a line number in a file link
  scrolls to and marks that line.
- Desktop: Settings > General has "Save files to", for choosing the folder
  saved files go to on macOS, Windows and Linux. Downloads stays the default.

### Changed

- Desktop: a coding agent notification names its project first, such as
  "org/web: Claude Code on build-box finished".
- Desktop: a relative file link opens the file from the worktree the agent
  works in, falling back to the session's folder.

### Fixed

- Desktop: each project remembers its selected coding agent, model and thinking level.

### Performance

- Desktop: a picture, video, 3D model or PDF opened from a chat downloads
  once and opens from the local copy after that while it is unchanged. An
  interrupted download resumes. Closing the session deletes the files only it
  opened, and files not opened for a day are removed.

## [0.8.16] - 2026-11-12

### Added

- Desktop: the machine page offers updates for coding agents (Codex, Claude
  Code, pi, OpenCode, Gemini) next to gh, teacli and flsh, checked hourly
  against their newest release.
- Desktop: the usage pill in the header shows Claude's weekly model limits,
  such as Fable, next to the plan's own percentage.

### Changed

- Desktop: an agent on another machine can send files to a desktop instead of
  being refused.

### Fixed

- Desktop: a terminal in a pane that gets narrower fits the pane again
  instead of cutting off the right side of its lines.
- Desktop: the terminal cursor stays visible while you type or output
  arrives, and a cursor a program sets to steady no longer blinks.
- Desktop: a file attached to a new agent shows its upload progress in grey
  instead of red, reaches 100%, then says the agent is starting.

### Performance

- Desktop: working agents, running commands, CI runs and research show a still
  dot instead of a turning spinner, so an idle window with agents at work no
  longer redraws every frame. The thinking sweep stops while the window is
  hidden, and a terminal cursor blinks only while its window has focus.
- Desktop: the built-in node runs git less often while agents work: a git
  command no longer makes it rewatch every folder and read them all again,
  file sync reads the checked-out commit once per change, and a folder's file
  list waits longer between reads while it stays the same.

## [0.8.15] - 2026-11-12

### Changed

- Desktop: "Sync coding agent skills" is off until you turn it on.
- Desktop: a chat code block that holds only one web address, with no language or as text, opens it when clicked, like inline code.

### Performance

- Desktop: opening a session from the board and going back start their move at
  once instead of stalling first on a long session.

## [0.8.14] - 2026-11-12

### Changed

- Desktop: terminals are drawn with Ghostty's engine on the GPU, so
  full-screen programs such as btop, neovim and yazi look and respond as in
  Ghostty, with Kitty images, Nerd Font icons, Powerline shapes and block
  graphics that meet across cells.

### Fixed

- Desktop: the spinner on the "commands running" line above an agent's message
  field turns steadily instead of starting over when the line is drawn again.

## [0.8.13] - 2026-11-12

### Fixed

- Desktop: pull requests and issues on a git host this Mac cannot reach are read through another
  machine with repositories on that host, and "Could not reach" shows only when no machine can.
- Desktop: a git host no machine reaches is tried again every few minutes instead of on every
  refresh, and its failures no longer fill the log.
- Desktop: renaming a coding agent session shows the new title at once in the
  open session, the session lists and the Option+Tab switcher, also on other
  desktops and phones, instead of after reopening the session.

## [0.8.12] - 2026-11-12

### Changed

- Desktop: a new project on GitHub or Forgejo clones into `<folder>/<name>`
  instead of `<folder>/<owner>/<name>`.

### Fixed

- Desktop: Settings, Appearance highlights the picked Board or List.

## [0.8.11] - 2026-11-12

### Added

- Desktop: Home opens on a board with four lanes: Needs you, Working, Ready to
  review and Done today. A project opens on the same board for its own work.
- Desktop: opening a session from the board grows its card into the session,
  and going back shrinks it again. Option-Tab slides to the next conversation,
  and a card that changes lane travels there. None of it plays with Reduce
  motion on.
- Desktop: Settings, Appearance picks Board or List for Home and for
  projects, each on its own. List brings back the sections and rows from
  before the board.
- Desktop: New project has a folder for each machine, filled in with its
  Projects in folder and offering its folders as you type.
- Desktop: a conversation has a thin scrollbar again, with a tick at each
  message you sent. Click a tick to jump to that message, or drag the thumb.
  Hovering a tick shows that message.

### Changed

- Desktop: Running left the sidebar; the Working lane lists what runs.
- Desktop: a project's sections open from its status strip, which gained
  Sessions. The list of sections shows beside a section, not on the board.
- Desktop: a new or imported project goes in each machine's Projects in
  folder as `<folder>/<org>/<repo>`, instead of a fixed `projects` folder.

### Fixed

- Desktop: an offline machine shows when it was last connected, also after
  the desktop restarts, instead of when it joined.
- Desktop: a machine that does not list its sessions in time keeps its rows
  in Agents and is asked again, instead of its sessions vanishing until the
  next change.
- Plan usage shows the weekly limits of single models, such as the Fable only week, next to Opus and Sonnet.

### Performance

- Desktop: an idle desktop starts far fewer background processes. The
  machine's tool and sign-in check runs once for every open connection,
  path checks no longer run `git` for each removed worktree, and idle
  session process and open-file checks run less often.
- The search for checkouts skips media, app and cache folders such as Pictures, Music, Movies and Library, so it is quicker and macOS does not ask for photo or music access.

## [0.8.10] - 2026-11-12

### Changed

- Desktop: sessions are copied when you continue them on another machine
  that is online, no longer in the background. Continuing a session from a
  machine that is off is no longer offered.
- Desktop: a web address written as inline code in a chat reply or prompt is now
  a link, including addresses with an IPv6 host.

## [0.8.9] - 2026-11-12

### Performance

- Desktop: copying other machines' sessions in the background no longer
  causes CPU spikes every minute when the copy cache is full.
- Desktop: a GitHub or Forgejo host that a machine cannot reach is no longer
  asked again for every issue and pull request list. Beep waits a minute
  before trying it again on that machine, then longer up to 15 minutes, so
  startup no longer spends minutes of CPU on failed host commands.
- Desktop: the palette's prompt search reads only a session's new
  transcript entries, once the session has been quiet for a minute, instead
  of rereading whole transcripts every 30 seconds. A slow link keeps the
  pages it already read, and a session that fails to load waits longer
  before the next try.

## [0.8.8] - 2026-11-12

### Added

- Desktop: when a coding agent has replied but a background subagent or
  command still runs, the session shows what it waits for, such as "Waiting
  for 1 subagent · Upgrade all deps · 12 min", in place of "Working". A
  click opens that subagent or the Running tab.

### Changed

- Desktop: a Claude Code turn that was cut off shows a grey "Interrupted" note instead of a message from you.
- Desktop: the Option+Tab switcher lists a session's subagents indented under
  it, in the order they started, each with its own state. A quick Option+Tab
  still returns to the last conversation.
- Desktop: an agent review session is named `Review #<number>` and keeps that
  name. A pull request an agent already reviewed shows Open review, which opens
  that conversation from any window or machine; Review again in the row's menu
  sends the next review to the same session.

### Fixed

- Desktop: spinners and live updates in a window no longer stop when another
  Beep window, such as the bar, hides.

## [0.8.7] - 2026-11-12

### Added

- Desktop: ⌘O in the bar opens the agent session in the main window, also
  offered in the hint row.

### Changed

- Desktop: Coding agents lists live sessions first and folds ended ones under
  a closed Ended group with a count. Open it to see them, each with when it
  last ran. The summary counts live sessions only.
- Desktop: an agent session opened in the bar starts with its Files and
  Changes pane folded. Click the strip or press ⌘2 to show it.

### Fixed

- Desktop: typing `/` or `@` in a project's agent start field, or on the new
  agent page, offers the agent's commands and skills and the project's files,
  as a running session's field does.
- Desktop: a file moved without `git add` shows in the diff as a rename instead of a deleted file and an added one.
- Desktop: on a flaky network the app no longer climbs to several cores of CPU and gigabytes of memory; the networking library kept retrying the same connection paths in a queue that never stopped growing.

### Performance

- Desktop: a project that cannot sync on a machine (uncommitted changes, or
  ahead of the others) no longer keeps machines busy retrying; they wait
  until it changes.
- Desktop: synced files under 16 KB are no longer copied into the sync database, which kept it large and held much of it in memory.

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
