# Android changelog

Earlier releases predate this changelog.

## [0.2.5] - 2026-11-14

### Added

- Android: Settings, Credentials has API keys for OpenRouter, Meshy, ElevenLabs
  and other providers. Every desktop shares them, and each machine gives them
  to flsh as env vars for its image, video, 3D and speech tools.
- Android: Home and each project open on a board of what needs you, what is
  working, what is ready to review and what finished today.
- Android: choose Default, Recent activity or Last opened when sorting projects. The choice is remembered.
- Android: rename projects from the project menu, with an option to rename their hosted repository while keeping local folder paths.
- Android: select repository copies on GitHub and Forgejo separately when renaming a project, and retry failed hosts while keeping successful renames.
- Android: subagent lists show the full model name from the chat model picker when it is known.
- Android: sort Home projects by recent activity or last opened, with a separate preference from Projects.
- Android: prompts, agent responses, and tools show discreet timestamps when available.
- Android: open uploaded attachments from messages, and play video from the original local copy on the uploading device while that copy is available.
- Android: an unfolded foldable (such as a Galaxy Z Fold) or a tablet
  shows the desktop layout with the sidebar instead of the stretched phone
  layout. Folding it switches back to the phone layout. A long press opens
  an item's menu in this layout.
- Android: a machine's page shows its graphics cards with their video memory.
- Android: pages slide in from the right, Back slides them in from the left, and tabs fade. Reduce motion turns it off.

### Changed

- Android: terminals are drawn with Ghostty's engine on the GPU, so
  full-screen programs such as btop, neovim and yazi look and respond as in
  Ghostty, with Kitty images, Nerd Font icons, Powerline shapes and block
  graphics that meet across cells.
- Android: Agents and Home say "Connecting to your machines…" until a machine
  answers, and "No machine is online." when none does, instead of "No
  sessions running."
- Android: Settings shows Agent accounts, where you set the Claude Code token
  and the Codex API key your machines use.
- Android: in landscape the tabs move to a rail on the left and the top bar and
  session head are shorter, so pages keep the height instead of looking like a
  small desktop. A session's Files and Changes stay full-screen tabs beside
  Log instead of a panel on the right.
- Android: a coding agent notification names its project first, such as
  "org/web: Claude Code on build-box finished".
- Phone: a picture attached to a coding agent message uploads in full, like other files,
  and the agent gets a small preview with the message. It is no longer shrunk.
- Android: a smaller project rename dialog shows repository settings only when selected.
- Android: Home and Projects use a styled sort menu.
- Android: child conversations stay in the parent agent's Subagents section and no longer appear in conversation lists or completion notifications.
- Phone: the conversation tab of an agent session is named Chat instead of
  Log.
- Phone: the Machines tab and the lists under the projects use the same
  new layout, in one column, with the actions always visible.
- Phone: while you type in an agent conversation, the top bar, the session
  details and the tab strip step aside, so more of the chat stays visible
  above the keyboard.
- Phone: the project page has no bottom task bar, so it uses the full
  screen. A + button in the top bar opens a new agent for the project,
  showing the same sunrise as desktop, and Work on it, Fix this, Continue
  and Agent review open it with the task filled in.
- Phone: on the project page, the GitHub and Forgejo links moved from the
  top bar into the More menu, so the project name has more room.
- Android: pairs with the new short QR code from the desktop. Scanning
  uses a new camera screen that reads codes off a monitor.
- Android: Add a project and New agent are a round button floating at the
  bottom right of the page instead of a small button in the top bar.
- Android: the model picker search matches the thinking level by its start, as
  it matches models: `opus 5.5 h` or `deepseek v4 med` shows rows such as
  `Opus 5.5 · High`. A version typed as `5.5` also finds ids written `5-5`.
- Android: diagrams in chats use the chat's colors instead of a white card, show at full size, and say what kind of diagram they are. Copy image, PNG and SVG moved into an Export menu next to Source and Copy. Sequence diagrams show each participant once, at the top.
- Android: the project menu (⋯) groups its sync settings under "Sync on all machines" as checkmarks instead of "On/Off (all machines)" labels. Choose ignored folders shows only while ignored folders sync, the Syncthing switch reads "Use Beep sync instead of Syncthing" (or back), and Remove project comes last after a line. All projects left the menu; Back goes there.
- Phone: takes the desktop's new look. Labels, tabs, buttons and details
  use the monospaced face, cards have no outlines, section labels are
  small, and a faint painted dawn band runs across the top of Home and
  Projects. The "Add a project" row at the end of Projects is gone. The +
  button in the title still adds one.
- Phone: in an agent conversation, the model button shows the agent's
  mark next to the message field.

### Fixed

- Android: a single tap on Send or Start with the keyboard up sends, instead
  of only hiding the keyboard.
- Android: an offline machine shows when it was last connected, also after
  the app restarts, instead of when it joined.
- Android: after coming back from the background or moving to another network,
  the app dials every machine again instead of sometimes never
  reconnecting. A machine that does not answer keeps its sessions on the
  list, marked connecting, and is asked again.
- Phone: pinching a picture zooms the picture, and the app itself no longer
  zooms. Drag to pan a zoomed picture and double tap to fit it again.
- Android: each project remembers its selected coding agent, model and thinking level.
- Android: tapping a file in a session opens it on screen instead of only adding
  its tab beside Log.
- Android: opening a session no longer focuses the message field, so the first tap
  in the log is not lost to the keyboard layout.
- Android: time filters keep their labels on one line, and the Files pane drops
  the desktop keyboard hint.
- Android: session message headers show the source project and session title instead of raw session tags or IDs.
- Android: file and attachment uploads keep running while bytes transfer, and time out only after 30 seconds without progress.
- Android: blocked conversations show the provider reason alongside any partial reply, including in saved history.
- Android: Agent chat sends automatically after attachments and image previews finish uploading; failed uploads keep the draft for retry.
- Android: View and remove attachments in a compact composer window with image previews; long sent file lists fold.
- Android: automatic session titles are shorter and no longer show internal tags.
- Android: attachments and media can be uploaded, opened and downloaded without the previous per-file size limits.
- Android: submitted prompts remain visible and greyed out while attachments upload, then send automatically.
- Android: switching conversations no longer stops uploads or sends files to the wrong chat.
- Android: a submitted message sends to the conversation it was written in, even after its attachments finish uploading.
- Android: conversations already at the bottom stay there when sending images.
- Android: interruption notices appear as status instead of becoming part of the next user prompt.
- Android: image prompts sent while a resumed agent starts no longer need to be retried.
- Phone: in agent conversations, the model button always shows next to
  the message field. A first tap opens the model picker, including while
  typing. Before, the button showed only while typing, and a tap on it
  missed.
- Phone: an agent conversation with two or more pictures or files open
  now opens on its chat. Before, it opened on the last picture every
  time.
- Phone: with the keyboard up, the model menu in a session opened near the
  top of the screen, partly cut off. It now stays right above its button.
- Phone: the project page left a blank band above the tab bar. It now
  runs down to the tab bar.
- Phone: an attached image opened full screen had its Close button
  under the status bar, so it could not be closed. The header now sits
  below the status bar.
- Android: the app no longer opens to a blank white screen.
- Android: the app no longer closes when Scan QR code opens the camera
  to pair a phone.
- Android: Open on a pull request or issue, and other web links in the app, open the browser instead of doing nothing.
- Phone: Return in the new agent composer starts a new line instead of sending the task; the Send button sends.
- Android: the back button goes back a page, or closes an open menu or dialog, instead of closing the app.
- Android: the app could take very long to connect, or never connect, after the phone switched networks or woke up, because it could not see the phone's network state.
- Android: the app no longer waits on every machine for project sync details at start.
- Android: uploaded attachments in project folders no longer appear as files made by the agent.
- Android: a refused pause, resolve or start-over of file sync shows its
  error.
- Android: Send in a review works again after a failed send.
- Android: a failed command shows the same exit code in every row.
- Android: Home shows Gemini CLI, pi and flsh by name.
- Android: a session in a nested project is counted under the same project
  everywhere.
- Android: a node connection that goes silent is noticed and opened again.
- Android: closed sessions appear in history as soon as the machine reports they ended.
- Android: App labels and controls no longer select text accidentally; chat messages and editable fields remain selectable.
- Android: Claude API errors in conversations appear in yellow again.
- Android: the code block Copy button stays at the right edge while scrolling horizontally.
- Android: a connection to a machine that stops answering is dropped and dialed again after about 15 seconds instead of 30.
- Android: machines reconnect within seconds after the phone wakes instead of waiting out connections that died while it slept.
- Android: the project page's ⋯ menu no longer closes the moment it opens.
- Android: Projects no longer scrolls sideways when an open issue or pull request has many labels; the labels wrap under the title.
- Android: the project page has no blank band above its task field.
- Android: with more than 500 sessions in the lists, the app no longer asks every machine where each session works over and over.
- Phone: tapping + in a session's message field opens the file picker instead of
  hiding the field's buttons.

### Performance

- Android: opening a session from the board and going back start their move at
  once instead of stalling first on a long session.
- Android: a picture, video, 3D model or PDF opened from a chat downloads
  once and opens from the local copy after that while it is unchanged.
  Closing the session deletes the files only it opened, and files not opened
  for eight hours are removed.
- Android: reads asked for once no longer stay live on the machines, and live views no one reads again are dropped after half a minute.

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

