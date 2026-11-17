# iOS changelog

Earlier releases predate this changelog.

## [0.3.0] - 2026-11-17

### Added

- iOS: choose Default, Recent activity or Last opened when sorting projects. The choice is remembered.
- iOS: rename projects from the project menu, with an option to rename their hosted repository while keeping local folder paths.
- iOS: select repository copies on GitHub and Forgejo separately when renaming a project, and retry failed hosts while keeping successful renames.
- iOS: subagent lists show the full model name from the chat model picker when it is known.
- iOS: sort Home projects by recent activity or last opened, with a separate preference from Projects.
- iOS: prompts, agent responses, and tools show discreet timestamps when available.
- iOS: open uploaded attachments from messages, and play video from the original local copy on the uploading device while that copy is available.
- iOS: a machine's page shows its graphics cards with their video memory.
- iOS: rename any machine from its menu in the sidebar, on Home or in a
  project's machines, or with Rename on its settings page. Every desktop,
  phone and node in the mesh shows the new name.
- iOS: New assistant on Home opens the Assistant page, which lists the
  assistant's chats. Tap one to open it, or New chat to start one.
- iOS: a step of a session's plan shows how many subagents ran under it
  and opens to their rows; a row opens the subagent.
- iOS: a problem report now says how the phone reaches each machine: its
  relay and addresses, whether each machine is linked, and the address it
  tried, so a phone that connects to nothing can be diagnosed.
- iOS: an agent reply in a coding agent session has a Copy button
  that copies the reply's text. The button appears once the reply has
  finished.
- iOS: Codex fast mode and all available thinking levels.
- iOS: the search asks your online machines directly, and it finds
  agent replies too.
- iOS: view supported device video using native H.264 capture, with JPEG fallback.
- iOS: a board lane gives a light haptic tick each time it settles on the next card.
- iOS: Settings, Credentials has API keys for OpenRouter, Meshy, ElevenLabs
  and other providers. Every desktop shares them, and each machine gives them
  to flsh as env vars for its image, video, 3D and speech tools.
- iOS: Home and each project open on a board of what needs you, what is
  working, what is ready to review and what finished today.
- iOS: pages slide in from the right, Back slides them in from the left, and tabs fade. Reduce motion turns it off.
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

- iOS: desktop controllers without hosting remain mesh controllers and no longer appear as coding agent or file hosts, or as phones.
- iOS: Agents are instructed to use image content and the request when titling sessions, while preserving explicit titles.
- iOS: saving or opening remote media without a cached copy now streams it in bounded chunks and handles cancellation safely.
- iOS: node updates select minor releases.
- iOS: a smaller project rename dialog shows repository settings only when selected.
- iOS: Home and Projects use a styled sort menu.
- iOS: child conversations stay in the parent agent's Subagents section and no longer appear in conversation lists or completion notifications.
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
- iOS: pairs with the new short QR code from the desktop.
- iOS: Add a project and New agent are a round button floating at the
  bottom right of the page instead of a small button in the top bar.
- iOS: the model picker search matches the thinking level by its start, as
  it matches models: `opus 5.5 h` or `deepseek v4 med` shows rows such as
  `Opus 5.5 · High`. A version typed as `5.5` also finds ids written `5-5`.
- iOS: an agent reply's send time now sits at the top right of the
  reply instead of on its own line above it.
- iOS: a session's plan is one line above the message field, with how
  many steps are done and the one that runs. It opens to the steps.
- iOS: a problem report can be up to 95 MiB, up from 5 MiB, and a large
  one gets more time to upload.
- Phone: the board always shows Needs you, even when nothing is waiting
  for you.
- iOS: in Add a project, tapping anywhere on a found project's row
  selects or deselects it, not only its checkbox.
- iOS: the assistant's message field on Home, shown on first start,
  before any projects, works like the other agent message fields. A
  paste of more than 10 lines becomes one "Pasted text" pill that you
  can preview, expand or remove. Text after a blank line is outlined as
  code.
- iOS: while Beep is open on screen, it no longer sends system
  notifications. Notices still appear in the app's bell. Notifications
  resume when the app is in the background.
- iOS: starting a conversation from a project's empty agent view
  shows your message as the first message at once, with a line saying
  the agent is starting on its machine. If the start fails, the input
  comes back with what you typed and the reason.
- iOS: a conversation started from the project bar, Home, the
  assistant, Review with agent or Work on it opens with your message
  already in it, not an empty log until the machine answers.
- iOS: a new agent session opens at once when you send its first
  message from a project, Home, the assistant, Review with agent or
  Work on it. The app keeps the start on this device and starts the
  agent in the background. The session shows your message with what
  it is waiting for or doing under it.
- iOS: if the machine is offline, the start waits and runs when the
  machine comes online, also after the app restarts. A waiting start
  is listed with your sessions and can be cancelled.
- iOS: a start that fails shows the reason, with Retry and Cancel.
- iOS: project session history now loads all available sessions.
- iOS: Show the copy button only on the latest agent reply.
- iOS: Home's board is now a Needs you queue across the top, above three
  lanes: Working, Ready to review and Done today. Project rows are gone
  from Home. Each card shows its project's coloured dot and name.
- iOS: Working cards show what the agent is doing right now.
- iOS: a Ready to review card has a Close button that ends the session.
  It is always shown.
- iOS: answered agent questions disappear from notifications.
- iOS: the pickers under Work on it, Continue, Review with agent, Fix this
  and Fix with an agent ask in steps: machine, harness, model, then
  thinking, with nothing picked for you. Picking the thinking level
  starts the agent. Other pickers keep their one-screen layout.
- iOS: sessions on offline machines are not found by their text.
- iOS: search by text only covers sessions that ran inside a project
  imported on that machine. Sessions in other folders are still listed,
  but they are not found by what was said in them.
- Phone: takes the desktop's new look. Labels, tabs, buttons and details
  use the monospaced face, cards have no outlines, section labels are
  small, and a faint painted dawn band runs across the top of Home and
  Projects. The "Add a project" row at the end of Projects is gone. The +
  button in the title still adds one.
- Phone: in an agent conversation, the model button shows the agent's
  mark next to the message field.
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

- iOS: delegated agents and their status appear in flsh sessions when the harness reports them.
- iOS: media viewers reject incomplete or changing remote downloads instead of displaying partial files.
- iOS: paintings no longer crash on startup when visibility tracking is unavailable.
- iOS: minor release notes now include changes from preceding patch releases, even without pending entries.
- iOS: file and attachment uploads keep running while bytes transfer, and time out only after 30 seconds without progress.
- iOS: blocked conversations show the provider reason alongside any partial reply, including in saved history.
- iOS: Agent chat sends automatically after attachments and image previews finish uploading; failed uploads keep the draft for retry.
- iOS: View and remove attachments in a compact composer window with image previews; long sent file lists fold.
- iOS: automatic session titles are shorter and no longer show internal tags.
- iOS: attachments and media can be uploaded, opened and downloaded without the previous per-file size limits.
- iOS: submitted prompts remain visible and greyed out while attachments upload, then send automatically.
- iOS: switching conversations no longer stops uploads or sends files to the wrong chat.
- iOS: a submitted message sends to the conversation it was written in, even after its attachments finish uploading.
- iOS: conversations already at the bottom stay there when sending images.
- iOS: interruption notices appear as status instead of becoming part of the next user prompt.
- iOS: image prompts sent while a resumed agent starts no longer need to be retried.
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
- iOS: Open on a pull request or issue, and other web links in the app, open the browser instead of doing nothing.
- Phone: Return in the new agent composer starts a new line instead of sending the task; the Send button sends.
- iPhone: the app no longer waits on every machine for project sync details at start.
- iOS: "Report a problem" now includes the phone's own log, so a report can
  be sent when no machine is connected.
- iPhone: the model picker opened while typing stays whole on screen above the composer instead of being cut off at the top.
- iOS: the top bar on the first page of each tab has a Settings button,
  so Settings and Report a problem open even when no machine is listed.
  Before, Settings opened only from a machine's row.
- iOS: Settings can now be closed with a Close button.
- iOS: the Report a problem sheet fits the screen; its buttons are no
  longer cut off.
- iOS: a problem report holds each machine's whole log for the chosen
  period, the phone's included, instead of the newest 1 MiB of each.
- iOS: the phone no longer keeps asking itself for events, which filled
  its log with warnings.
- iOS: the plan line above the message field shows its status as a dot again instead of a long blue streak.
- iOS: terminals now use the colours of the picked skin: background,
  text, cursor, selection and the main ANSI colours. With a light skin the
  terminal is light too instead of staying dark, and open terminals change
  at once when the skin changes.
- iOS: the Copy button under a quote at the end of an agent reply
  appears once the reply finishes, not while the quote is still
  streaming in.
- iOS: the output of an opened tool call can be selected again.
- iOS: a conversation just started no longer drops the message you
  sent and shows "Starting the agent" for a few seconds while its
  machine sets the model and thinking level.
- iOS: the project board no longer loops while it measures its lanes.
  Before, the whole window could go blank after the phone woke up
  offline or after a session view closed.
- iOS: when a part of the window fails to draw, only that part shows
  "This part failed to draw" with a Try again button. The rest of the
  window keeps working. Before, the whole window went blank until you
  restarted the app.
- iOS: sent images retain their detail and reopen at full quality.
- iOS: SVG files open in the image viewer and can be previewed from Files.
- iOS: a Markdown file previewed in a file tab scrolls again instead of cutting off below the first screen.
- iOS: image-only agent messages now include a clear default prompt instead of using filenames as the task.
- iOS: on Wi-Fi networks that rewrite DNS answers, such as some hotel
  content filters, the phone now reaches machines. Names resolve through
  encrypted DNS first, then the network's DNS if that fails.
- iOS: project card times count earlier agent sessions, and Recent
  activity now sorts to match. The order also refreshes after a tap.
- iOS: the usage pill at the top no longer keeps Claude's plan percentages
  from an earlier run while Codex's numbers update. Claude's current numbers
  replace the old ones as soon as a machine reads them.
- iOS: uploaded attachments in project folders no longer appear as files made by the agent.
- iOS: a refused pause, resolve or start-over of file sync shows its error.
- iOS: Send in a review works again after a failed send.
- iOS: a failed command shows the same exit code in every row.
- iOS: Home shows Gemini CLI, pi and flsh by name.
- iOS: a session in a nested project is counted under the same project
  everywhere.
- iOS: a node connection that goes silent is noticed and opened again.
- iOS: closed sessions appear in history as soon as the machine reports they ended.
- iOS: resumed Claude sessions recover available earlier messages from native history.
- iOS: App labels and controls no longer select text accidentally; chat messages and editable fields remain selectable.
- iOS: Claude API errors in conversations appear in yellow again.
- iOS: the code block Copy button stays at the right edge while scrolling horizontally.
- iOS: image attachments no longer become conversation titles when a session has no name.
- Phone: the notifications and usage panels fit the screen under the top bar instead of being cut off on the left.
- iOS: the spinner on the "commands running" line is a small circle again instead of a stretched slash.
- iOS: the spinner on the plan line above the composer is a small circle again instead of a stretched slash.
- iOS: On tablets and unfolded foldable phones, which show the desktop
  layout, the board's columns scroll one card at a time when you drag them.
- iOS: On tablets and unfolded foldable phones, which show the desktop
  layout, the new task composer shows its send button.
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

- iOS: builds compile fewer dependencies.
- iOS: the phone no longer downloads transcripts to build a search
  index, which saves data, battery and storage.
- iOS: opening a session loads only the newest entries that fit the window,
  plus a few more, instead of the last 200. It loads with less data.
  Older entries still load as you scroll up.
- iOS: remote sessions appear faster.
- iOS: opening a session from the board and going back start their move at
  once instead of stalling first on a long session.
- iOS: a picture, video, 3D model or PDF opened from a chat downloads
  once and opens from the local copy after that while it is unchanged.
  Closing the session deletes the files only it opened, and files not opened
  for eight hours are removed.
- iOS: reads asked for once no longer stay live on the machines, and live views no one reads again are dropped after half a minute.
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

