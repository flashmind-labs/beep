# iOS changelog

Earlier releases predate this changelog.

## [0.2.2] - 2026-11-13

### Added

- iOS: Settings, Credentials has API keys for OpenRouter, Meshy, ElevenLabs
  and other providers. Every desktop shares them, and each machine gives them
  to flsh as env vars for its image, video, 3D and speech tools.
- iOS: Home and each project open on a board of what needs you, what is
  working, what is ready to review and what finished today.
- iOS: pages slide in from the right, Back slides them in from the left, and tabs fade. Reduce motion turns it off.

### Changed

- iOS: terminals are drawn with Ghostty's engine on the GPU, so
  full-screen programs such as btop, neovim and yazi look and respond as in
  Ghostty, with Kitty images, Nerd Font icons, Powerline shapes and block
  graphics that meet across cells.
- iOS: Agents and Home say "Connecting to your machines…" until a machine
  answers, and "No machine is online." when none does, instead of "No
  sessions running."
- iOS: Settings shows Agent accounts, where you set the Claude Code token
  and the Codex API key your machines use.
- iOS: in landscape the tabs move to a rail on the left and the top bar and
  session head are shorter, so pages keep the height instead of looking like a
  small desktop. A session's Files and Changes stay full-screen tabs beside
  Log instead of a panel on the right.
- iOS: a coding agent notification names its project first, such as
  "org/web: Claude Code on build-box finished".
- Phone: a picture attached to a coding agent message uploads in full, like other files,
  and the agent gets a small preview with the message. It is no longer shrunk.
- iOS: diagrams in chats use the chat's colors instead of a white card, show at full size, and say what kind of diagram they are. Copy image, PNG and SVG moved into an Export menu next to Source and Copy. Sequence diagrams show each participant once, at the top.
- iOS: the project menu (⋯) groups its sync settings under "Sync on all machines" as checkmarks instead of "On/Off (all machines)" labels. Choose ignored folders shows only while ignored folders sync, the Syncthing switch reads "Use Beep sync instead of Syncthing" (or back), and Remove project comes last after a line. All projects left the menu; Back goes there.

### Fixed

- iOS: when the keyboard goes away while a field still has focus, the page
  moves back into place and the tab bar returns, instead of staying pushed
  down under an empty band.
- iOS: a single tap on Send or Start with the keyboard up sends, instead of
  only hiding the keyboard.
- iOS: an offline machine shows when it was last connected, also after
  the app restarts, instead of when it joined.
- iOS: after coming back from the background or moving to another network,
  the app dials every machine again instead of sometimes never
  reconnecting. A machine that does not answer keeps its sessions on the
  list, marked connecting, and is asked again.
- Phone: pinching a picture zooms the picture, and the app itself no longer
  zooms. Drag to pan a zoomed picture and double tap to fit it again.
- iOS: each project remembers its selected coding agent, model and thinking level.
- iOS: Save on a video, picture or file works. It goes to Beep's folder in
  the Files app, and the share sheet opens so a video or picture can go to
  Photos.
- iOS: tapping a file in a session opens it on screen instead of only adding
  its tab beside Log.
- iOS: opening a session no longer focuses the message field, so the first tap
  in the log is not lost to the keyboard layout.
- iOS: time filters keep their labels on one line, and the Files pane drops
  the desktop keyboard hint.
- iOS: session message headers show the source project and session title instead of raw session tags or IDs.
- iOS: a connection to a machine that stops answering is dropped and dialed again after about 15 seconds instead of 30.
- iOS: machines reconnect within seconds after the phone wakes instead of waiting out connections that died while it slept.
- iOS: Projects no longer scrolls sideways when an open issue or pull request has many labels; the labels wrap under the title.
- iOS: the project page has no blank band above its task field.
- iOS: with more than 500 sessions in the lists, the app no longer asks every machine where each session works over and over.
- Phone: tapping + in a session's message field opens the file picker instead of
  hiding the field's buttons.

### Performance

- iOS: opening a session from the board and going back start their move at
  once instead of stalling first on a long session.
- iOS: a picture, video, 3D model or PDF opened from a chat downloads
  once and opens from the local copy after that while it is unchanged.
  Closing the session deletes the files only it opened, and files not opened
  for eight hours are removed.
- iOS: reads asked for once no longer stay live on the machines, and live views no one reads again are dropped after half a minute.

## [0.2.1] - 2026-11-11

### Added

- iOS: a message sent to a session whose machine cannot be reached waits on
  this device, in the session's queue marked "waiting for connection", and
  goes in the order typed once the machine is back, even after the app
  restarts. Edit or remove it like other queued messages; one the machine
  refuses stays with the reason.
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
- iOS: fold a group on the Projects page with a tap on its name. It
  stays folded on this phone; a search still shows its matches.

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
- iOS: each row on the Coding agents page shows its harness icon, and
  New sits in the page header instead of a second title under it.
- iOS: a session's message field and its buttons sit in one rounded
  box, without a border around the field itself.

### Fixed

- iOS: buttons are compact (Import, Reply, Review and the others) instead of full touch-target height; rows and menus keep their larger targets.
- iOS: an open session no longer stays on "Lost the connection...
  Reconnecting..." until the app restarts. When a machine stops taking new
  reads while still connected, as after switching between mobile data and
  Wi-Fi or coming back from the background, the app connects to it again
  and the session catches up on its own.
- iOS: the Agents list shows sessions that start, end or change while it is open, without leaving the page and coming back.

- Projects: a long group name stays on one line and ends in "..." instead of wrapping over several lines.
- iOS: on opening or waking, sessions, Home and machines shown from what
  the app kept now say they are still connecting. A machine not reached yet
  has a hollow dot and reads "connecting", its sessions are dimmed, and an
  open conversation says "Connecting to" its machine until the first live
  update arrives. Before, the kept view looked live until it suddenly
  changed.
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
- iOS: the Projects and Coding agents pages no longer scroll
  sideways or stay shifted left after swiping back, and long session
  titles end in an ellipsis.
- iOS: a session's branch chip no longer starts with a stray dot.

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

