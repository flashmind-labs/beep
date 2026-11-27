# Desktop changelog

Pending changes are files in [unreleased/desktop](unreleased/desktop), collected
here by version and date at release. Releases before 0.6.0 predate this changelog.

Other apps have independent versions and changelogs:
[node](node.md), [Android](android.md),
[iOS](ios.md), and [server](server.md).

## [0.10.43] - 2026-11-27

### Added

- Desktop: Open linked Beep sessions directly from chat messages (`beep://session/<id>` or `beep://session/<id>@<machine>`, plus inline code spans matching known session IDs), with Cmd/Ctrl-click or right-click to open beside, and a Copy link item in the session menu.

### Fixed

- macOS: Beep no longer crashes at launch (0.10.42).
- Desktop: macOS Notification Center now refreshes its cached app icon when Beep updates to a new icon, instead of staying on the old one.

## [0.10.42] - 2026-11-26

### Fixed

- Long conversations open at the latest message at once on desktop and read earlier messages as you scroll up, instead of loading the whole history first.
- Scrolling up while a conversation loads no longer snaps back to the end, and a conversation keeps its place when the window is hidden and shown.
- Desktop: Antigravity and Hermes sessions hide the composer mode pill while in their starting full-access mode, showing it only after switching to Plan, Accept edits or another mode.
- Opening Home's PRs and issues tab with many authored pull requests no longer freezes the desktop app at 100% CPU, and row actions load when hovered or focused.

## [0.10.41] - 2026-11-26

### Changed

- Desktop: the device, model clip, provider machine, credential provider and MCP server pickers open themed menus instead of the system menu. Offline machines show as Offline, and devices are grouped by machine.
- Desktop: on macOS, Windows and Linux, session pages, the open session and agent pane tabs now arrive as they change instead of the window asking again and again. Leaving a session view clears it.
- Desktop: on macOS, Windows and Linux, removing a synced folder and Codex sign-in update as soon as the machine reports the change, instead of waiting for a timer.
- Desktop: the plan usage popover groups each plan in a card, keeps limit names on one line with full-width bars, and labels its tables more clearly.

### Fixed

- Desktop: fixed the Voice dictation model picker wrapping multi-word model names and stretching the API key chip in Settings.

## [0.10.40] - 2026-11-26

### Fixed

- macOS: fixed a release build link failure that kept the local voice dictation update from publishing.

## [0.10.39] - 2026-11-26

### Changed

- Desktop windows draw Home, project pages and file views from state pushed to them, with fewer requests between parts of the app on macOS, Windows and Linux.

## [0.10.38] - 2026-11-25

### Added

- Desktop: hold Option (Alt on Windows and Linux) while a prompt composer is on screen for push-to-talk voice dictation, and release to finish.
- Desktop: on-device real-time voice dictation with Parakeet Unified and Nemotron 3.5, with one-click model downloads in Settings and a live-only dictation model menu.
- Auto-compact settings now show for Antigravity on desktop.

## [0.10.37] - 2026-11-25

### Fixed

- Desktop: the prompt composers grow taller during voice dictation so the timer and voice animation stay clear of the text and controls, then shrink back when dictation ends.
- Desktop: words you edit or delete while dictating stay edited when you keep speaking, instead of being overwritten by the earlier transcript.
- Desktop: typing a folder path in Import a project's Clone into field now clones into that folder instead of appending `<org>/<repo>`, checks Syncthing for it, and offers Default to return to the machine's projects folder.

## [0.10.36] - 2026-11-25

### Added

- Machines running an older beep-node update themselves as soon as they come online, once their agent sessions are idle.
- Desktop: a setting, "Update machines automatically", turns off the automatic beep-node updates of your machines. A notification says when one is behind instead.

### Fixed

- Desktop: an open session whose live link could not start now tries the link again every five seconds instead of rereading the session many times a second.
- Desktop: a session title you clear now shows the agent's own title at once in every list.
- Desktop: the sign-in sheet and Codex sign-in follow the machine's updates, so they show a finished or failed sign-in without delay.
- A long message no longer stays listed as sent after the session shows it.
- Background messages sent one after another each leave the sent list once the session shows them.
- Terminal and project session lists update from what machines push instead of asking every machine again.
- A waiting outside session leaves the running list when its day ends.
- Desktop: sessions started outside Beep keep updating on Home after a machine restarts or its connection drops.
- Desktop: a machine whose automatic update failed is no longer asked again on every change, only after it has been offline, and an update another desktop started is not retried.

### Performance

- Desktop: the Changes pane, Git history and a previewed page now read again only when a change concerns them, as soon as the machine says so, instead of on a two-second timer. Needs a machine on the new node version; older machines keep the old behaviour.
- Desktop: fewer reads of sessions, accounts, project machines, sync detail and reviews; they now arrive as updates from the machines.

## [0.10.35] - 2026-11-25

### Added

- Desktop: Continue a chat with another harness. Pick a model from another harness in the chat's model picker, or use Continue with in the session menu, and your next message starts the new harness in the same folder. The new session reads the earlier chat first, and the chat links both.

### Fixed

- Desktop: adding a project to another machine now shows the copy as it goes and closes the sheet when the copy is done.
- Desktop: a project that is copying keeps showing its progress on Home while no project page is open.
- Desktop: the model lists on the start form and in Settings refresh after a coding agent is updated, its settings are saved, or its account token is saved or removed, instead of staying as they were for up to five minutes.
- Desktop: the branch and worktree shown for a session in the sidebar, session lists and waiting bar now follow the session when it switches branch or moves to a worktree, not only for the open session.
- Desktop: the status dots in the new agent machine picker no longer touch the machine names.

### Performance

- Desktop: the bell takes sync conflicts from what the machines report, and no longer asks every machine again each time a sync changes.
- Desktop: folder views a window followed stop being read again once no window follows the folder.

## [0.10.34] - 2026-11-25

### Added

- Desktop: a session's Running tab lists the Docker containers its agent
  started, with their ports and an Open button for the page they serve.
  Stop ends a container and keeps it. Turn this off in Settings, General,
  under "Docker containers in Running".
- Dictate prompts by voice in the new-session composer and the reply field on macOS, Windows and Linux. Turn it on and pick a model in Settings > General. It uses your ElevenLabs, Mistral or OpenRouter API key, and ElevenLabs and Mistral show words live as you speak.
- Desktop: select text in an agent reply and click Quote (or press Q) to append it as a Markdown blockquote in the session message input.

### Changed

- Desktop: Antigravity notices that start with a warning sign, such as its quota message, now show in the warning color.
- Desktop: a message you send shows as a waiting row at once, instead of waiting up to a quarter second for the machine's first answer. One the machine refuses stays in the list with its reason.
- Desktop: the team invite code fits on one line with its Copy button, and the note below says who can join and for how long.

### Fixed

- Desktop: Stop, Stop all, Close and answering a question now go out at once while a message to that session is still uploading files, instead of waiting behind the upload. Close also drops the message that was uploading.
- Desktop: Stop, an answer or a list of waiting messages from a window still showing a session that has since been resumed now reach the resumed session, also after Beep restarts while its messages wait. A page opened on such a session goes on in the resumed one.
- The tools view no longer asks the npm registry again on every redraw after a lookup failed (macOS, Windows, Linux).
- Team cards keep updating when the machine acting for the team runs an older Beep node (macOS, Windows, Linux).
- A terminal's or task's final lines and its exit show without reopening it, including on older nodes (macOS, Windows, Linux).
- A team that could not be read when its project opened is read again when the projects change (macOS, Windows, Linux).
- Opening an agent that was just started waits for it even when other sessions change meanwhile (macOS, Windows, Linux).
- macOS, Windows and Linux: the session list no longer waits for a slow or offline machine's move progress before it shows.
- macOS, Windows and Linux: a session you moved, deleted or relocated and a child agent that was just started show from the pushed list, with no extra reads and no 5 second retry while a chat window waits for its chat.
- macOS, Windows and Linux: the sidebar groups, this computer's row and the Add a machine sheet follow the pushed machine state at once, and the sidebar marks the project you are viewing.
- macOS: a slow disk on this Mac no longer delays its status reports to other devices, and a team change no longer waits on a slow desktop.
- Team boards, team members and invites from contacts update as they change, without waiting for a timer (macOS, Windows, Linux).
- Machine clocks, terminal lists, each machine's coding agents and a project's machines update when they change (macOS, Windows, Linux).
- A session move shows its progress as the target machine reports it (macOS, Windows, Linux).
- Desktop: a message's files are no longer removed from disk before the record that drops them is saved, so a crash cannot leave a message whose files are gone.
- macOS, Windows and Linux: two files with the same name sent in different messages no longer open as each other's contents.
- macOS, Windows and Linux: a session resumed by a message keeps the model you picked when you sent it, even if you pick another before it resumes.
- Desktop: editing or removing a queued message while the agent works keeps the pictures of the other queued messages and puts the edited one's pictures back in the field.
- Desktop: a message with files still being read is kept by Beep from the moment you press Enter, so closing or reloading the window no longer loses what you typed. It shows as "reading files" in the queue.
- Desktop: Resume in a session's menu now runs in the background like a message does, shows its progress in the queue and keeps going after a lost connection.
- Desktop: a message you sent while the agent works stays in the queue as "after the current step" until the session lists it, then shows once. It no longer shows twice or vanishes when its text was changed by attached files.

### Performance

- Desktop: follows a node's session list by the sessions that changed, so Stop, queued messages and session views on a remote machine no longer wait behind the list.
- Desktop: the outbox logs each try at a queued message (check, send, upload, resume, configure) with its result and time, so a retry loop shows in the log.
- A project page whose run read is slow no longer holds back the other open project pages (macOS, Windows, Linux).
- The tray no longer keeps a second copy of every session push while all windows are hidden (macOS, Windows, Linux).
- The command palette builds its session list when it opens, not on every session change (macOS, Windows, Linux).
- The machine list no longer waits for the slow reads of each machine's agents and project pages (macOS, Windows, Linux).
- The sync and move notices no longer redraw views while every window is hidden (macOS, Windows, Linux).
- Team cards are followed only while a project's page shows them (macOS, Windows, Linux).
- macOS, Windows and Linux: team cards and invites arrive as they change instead of up to 10 or 60 seconds later, and the app no longer asks for them on a timer.
- macOS: the device list is read once per change, and terminal output no longer rereads the terminal list.
- The bell no longer rereads the mesh every few minutes: it reacts to changes the machines push (macOS, Windows, Linux).
- macOS, Windows and Linux: a project's checkouts, terminal lists, a terminal's view-only output, each machine's coding agents, machine clock warnings and move progress now arrive as they change, with no rereads and no 30 second clock timer.
- Desktop: model lists, folder listings, worktrees, session places, new-machine tickets and project sync details now arrive as pushes instead of repeated reads.
- macOS, Windows and Linux: the models each coding agent offers, a session's worktrees and places, a folder's file list, the `@` file menu, a project's sync detail and the Add a machine sheet now take their changes as they arrive, with far fewer reads while an agent works.
- Desktop: saving a message with a very large file no longer holds up saving the other queued messages behind it.

## [0.10.33] - 2026-11-24

### Changed

- Desktop: Replaced circular progress spinners with a calm Neural Spark dot-matrix indicator and added an inline 7x7 dot-matrix next to live thinking and activity lines.

### Fixed

- Desktop: a message with attached files keeps uploading and sending when its window closes or reloads. The files go to the machine behind a row above the field that says "uploading", and a failed upload stays as a row with its reason, where Edit puts the words and files back in the field.
- Desktop: a message to an ended session keeps sending when its window closes or reloads. The session resumes behind a row that says "resuming", the message goes once it runs, and the window opens the resumed session in the background. A second message sent meanwhile waits behind the first and never resumes the session twice.
- Desktop: a message sent right after dropping a file is no longer left behind in the field when you switch conversations. It still goes, in the conversation it was written in.

## [0.10.32] - 2026-11-24

### Fixed

- The tray keeps showing sessions that need you while every Beep window has been hidden for more than five minutes.

## [0.10.31] - 2026-11-24

### Added

- Desktop: Added a **Join team** button next to **Add a project** on the **Projects** page and in `⌘K` search so you can paste a `beepteam1` code without importing the project first.
- Desktop: Joining a team now imports a matching local checkout or clones the repository automatically when the project is not added yet.

### Fixed

- The tray's list of sessions that wait on you now follows changes as they arrive instead of checking every few minutes.
- Moving a session no longer makes every project page ask the machine again.
- A failed session list on one machine no longer makes the others list again.
- A machine you rename or remove on Home or the Machines page changes everywhere at once instead of after the next update.
- The agent bar no longer says "Install a coding agent" after a failed first read of your machines; it reads them again.
- Desktop: Home, the project lists, workflow runs, usage and agent accounts follow the latest update at once instead of asking again after each change you make.
- Desktop: Removing a project or a machine no longer brings back its old sessions, places or git state when you open the page again.
- A session move started on another desktop shows at once in the session lists.
- A project's session list keeps updating for as long as its page stays open.
- Plugin changes, and agent tokens saved, revoked or signed in on this desktop, show without a second read, so Settings no longer flickers back to old values.
- A message sent to an idle agent over a slow connection no longer shows a Steer row above the message field when it is already in the conversation.
- A message sent while its files are still uploading, or to an ended session that has to resume, now shows at once and the message field stays free for the next message, which is sent after it. A file still uploading shows "uploading" instead of a bar stuck at 0%.
- A pasted or dropped picture now crosses to the machine once, with the message, instead of being uploaded as a file and sent again. Sending a screenshot no longer waits on an upload, and the message no longer mentions a file copy of it. Larger or unusual pictures still upload as before.

### Performance

- Machines page no longer asks every machine for tool updates, update support and this computer's state on each change; machines running a current beep-node report them as they change.
- The Device tab on a machine running a current beep-node follows its devices without checking on a timer.
- Project pages that are no longer shown stop being rebuilt, and the palette no longer keeps a second copy of the session list.
- Desktop: Dragging and resizing the side pane draws it with the rest of the frame instead of forcing a separate redraw on every step.
- Home and the session lists read the agent moves only when a move changes, not on every update.
- The Machines page no longer asks for machine traffic again on each change.
- Desktop: Importing a project or a repository from open items no longer reads all of Home a second time.
- Desktop: The app keeps one copy of each project's sessions, places and git state, and lets go of the projects you used least.

## [0.10.30] - 2026-11-24

### Fixed

- Desktop: Local Pi sessions now expose Beep's MCP tools (`start_agent`, `list_machines`, `list_projects`, and other Beep tools) directly through Beep's Pi extension.

## [0.10.29] - 2026-11-24

### Changed

- Desktop: Grouped the Settings sidebar into App, Fleet, Agents, and Extensions sections, added a `/` search shortcut, moved coding agent configuration into tabs on the Agents page, and updated account cards with semantic status dots.

### Fixed

- Desktop: Local Pi sessions now recover from provider `content_filter` stops within the same turn and surface unrecovered provider errors instead of ending with blank output.
- Desktop: The project rail beside an open session now stays subscribed to project updates, so running Actions and counts update without returning to the project page.

## [0.10.28] - 2026-11-24

### Added

- Antigravity sessions started outside Beep can be continued from the session menu.
- Desktop and phone: Filter files in the Files tab by path segments (`folder/file`, `folder/folder`, or `folder/`).

### Changed

- Desktop and phone: Improved menu and navigation readability with labeled sections, aligned icons, inline toggle switches, sync mode tags, harness labels in the model picker, and clearer selection indicators in the command palette and model list.

### Fixed

- A project page no longer shows a machine's sync state from an old answer after the machine reconnects.
- A Git fetch you ask for now updates the ahead and behind counts even when another read finishes first.
- A machine that is slow to send its session list is read directly instead of showing old sessions.
- Messages and starts waiting to send are saved in the order they changed, so a quit or crash does not bring back a sent message.
- After a machine reconnects, its sessions show at once instead of waiting for a second read.
- A past session you move to another project stays on its new project page.
- A message that failed to send after you left its session no longer shows again when you reopen the session.
- A session opened beside another no longer sits on an old copy of its conversation when its live link fails to open.
- Machine names, agent hosts and the session list now update as soon as a change arrives, without a second read that could show an older state.
- A branch's ahead and behind counts and a project's session list no longer step back to older values after an update.
- Renaming a project, removing a machine from it or changing its folder no longer reloads all of Home.
- Desktop (macOS): Turn off native autocomplete, autocorrect, and auto-capitalization across text inputs and search fields.

### Performance

- A project page checks Syncthing on all machines at once, so a slow or unreachable machine delays its Syncthing row by 3 seconds at most instead of 3 seconds each.
- Project pages redraw their session lists only when that machine's sessions changed, at most once a second.
- Several Codex sign-ins waiting at once are checked together, and the check no longer runs on every line a process prints.

## [0.10.27] - 2026-11-24

### Added

- Desktop: pick thinking levels (Low, Medium, High, Extra high, Max; filtered per model when reported) when starting or switching models on Antigravity sessions.
- Desktop: Cmd+click (Ctrl+click on Linux and Windows) in the file viewer and
  editor, as well as `gd` in Vim mode, jumps to a symbol's definition using the
  machine's installed language server, offering a picker when multiple
  definitions match.
- Desktop: middle-click an idle session in the sidebar to close it.
- Desktop: Switch to an existing local branch from the project Git panel.

### Changed

- Desktop: Editor Vim mode preference now syncs across desktops via mesh state
  in addition to local storage, ensuring the choice persists across restarts and
  machines.
- Desktop: expand or collapse all Git panel sections with one control.
- Desktop: On a node too old for live sync status, the view may show no answer while a direct check is slow.
- Desktop: a session opened beside the conversation shows its transcript at once and updates as the session runs.

### Fixed

- Desktop: the project status strip now shows the Team button even when a project has no team yet, so you can open Team directly from the Board view to invite or join.
- Desktop: short metrics queries work even when older retained logs exceed the scan limit. Summaries show the observed times in UTC.
- Desktop: the `/` menu of a new chat lists an agent's commands from its last session, even when none of its sessions is running.
- Desktop: A deleted sub-agent or reviewer leaves the session list at once.
- Desktop: Files and Changes panes load from a slow machine instead of reporting a missing subscription snapshot.
- Desktop: A project's session history that cannot be read keeps the rows already shown.
- Desktop: Tasks no longer carry over from one session to another when you open a session.
- Desktop: Machine and agent lists now match on every screen.
- The model picker on desktop now shows only the harnesses a project allows, and no longer starts on one the project leaves out.
- Desktop: horizontally scrolled code in the file editor and file search no longer shows through behind the line-number gutter.
- Desktop: Show the highest longer-window usage in the header and label five-hour usage separately once it reaches 1%.

### Performance

- Desktop: The File sync view follows a machine's changes as they arrive, without polling.
- Desktop: Project pages follow session changes without asking the machine again.
- Desktop: Operations of different kinds on one machine now run together.
- Desktop: A split session tab shows its transcript at once and keeps it live.
- Desktop: A new session title appears at once in every list.
- Desktop: Settings opens straight away.
- Desktop: Removing a project or machine no longer redraws all of Home.
- Desktop: Session lists no longer reread on every agent update.
- Desktop: Project git state updates without a fresh read.
- Desktop: Terminal output is not read while no terminal is open.

## [0.10.26] - 2026-11-24

### Changed

- Desktop: pressing Escape from a threaded Assistant chat now returns to Home in two presses instead of multiple intermediate steps.

### Fixed

- Desktop: a question you answered leaves "Needs you" on Home and the tray
  at once, where it could stay for up to a minute.
- Desktop: a session you close on this Mac shows as ended at once in every
  list.
- Desktop: after a restart, the sessions of a machine that is still
  connecting stay on Home until it answers, where they used to disappear.
- Desktop: a deleted session does not come back in Home or the session lists
  while its machine is being listed.

## [0.10.25] - 2026-11-23

### Added

- Desktop: SVG and PDF downloads from HTML previews save on this device and open in a new Beep tab.

## [0.10.24] - 2026-11-23

### Changed

- Desktop: the right pane in Assistant appears when the chat has files or active threads.
- Desktop: assistant chats stay in creation order when chats or threads receive updates.

### Performance

- Desktop: the Running tab of a conversation shows its last list of commands
  and ports as soon as you switch back to it, and then keeps up to date
  from changes the machine reports instead of reloading after every change.

## [0.10.23] - 2026-11-23

### Fixed

- Desktop: Pi models show their provider beside the model name.
- Desktop: saved instructions now work with Pi and agents without system-prompt support.

## [0.10.22] - 2026-11-23

### Added

- Desktop: The diff viewer colours code by its language, in both the
  unified and side by side views.
- Desktop: The diff filter marks every match in the code and shows its
  position, such as 3/17. Enter and Shift+Enter, n and N, or the up and
  down buttons jump to the next or previous match, scrolling to it and
  unfolding its file.

### Fixed

- Desktop: Sidebar, project page and Done today lists no longer flash a
  shorter list while a slower machine answers. Lists keep showing until
  every machine replies; a failing machine keeps its sessions.
- Desktop: a long project name in Home's idle list is cut with an ellipsis
  instead of running into the time column.
- Desktop: new agent launches keep their account and attachments when navigating.
- Desktop: Stop All restores queued messages to the correct session and finishes after navigation.
- Desktop: closing a terminal pane keeps it detached.
- Desktop: queued machine list refreshes run as requested.
- Desktop: cancelling Claude Code sign-in no longer cancels the next attempt.
- Desktop: failed plugins stop consuming CPU, and interrupted plugin updates restore the previous version.

## [0.10.21] - 2026-11-23

### Added

- Desktop: teams per project. Invite people who can push to the
  project's repository with a code that is valid for a day. They join
  from the Team page, and joining proves they have push access to the
  repository.
- Desktop: the project board shows teammates' agent cards beside yours,
  with a filter for everyone, mine, or one teammate. A teammate's card
  opens its issue or pull request, never their session.
- Desktop: the Team page lists members and their machines, the invite
  code, and contacts to invite to other projects. Switches let you pick
  up assigned issues and let teammates' agents message yours. You can
  leave, remove a member, or allow them back.
- Desktop: removing a contact from the Team page takes them off every
  team you share with them.
- Desktop: issues show who they are assigned to, with Mine and
  Unassigned filters and an Assign menu. In a team project, Work on it
  assigns the issue to you first, and asks before taking one someone
  else holds.
- Desktop: Needs you lists teammates' questions, invites from contacts,
  and issues that could not start or ended without a pull request.

### Changed

- Desktop: detached project and assistant windows keep plan usage in the header at narrow widths, show local requests for input in their lists, and leave global notifications in the main window.
- Desktop: agent edit diffs in the chat use the same syntax colours as the file viewer, with the +/- marks coloured.
- Desktop: the file editor colours types, functions, macros, properties, tags, attributes, constants, headings and links, not only keywords, strings, comments and numbers.

### Fixed

- Desktop: switching projects no longer leaves draft text over the task placeholder.
- Desktop: double or triple clicks on timestamps or between messages no longer select conversation text.
- Desktop: show Antigravity's five-hour quotas alongside its weekly quotas when reported.
- Desktop: detached chats load automatically when their machine reconnects after an update, and HTML tabs reopen in their saved preview or source view.
- Desktop: Delivered messages no longer reappear as sending in the chat queue.
- Desktop: Antigravity sessions start and resume with saved instructions.
- Desktop: attachments in the first message of a new Assistant or project chat use the machine cache, leaving project folders untouched.
- Desktop: paste images directly into chats on Linux.
- Desktop: agents on enrolled nodes can start linked agents on a desktop when machine sharing allows it.
- Desktop: Files and Changes follow a Flashmind session’s worktree again after
  a node restart.
- Desktop: the project session list stays scrollable without a visible scrollbar.
- Desktop: in a narrow window, folders under "Found on your machines" on Home no longer draw their machine names over their agents and activity. Each folder takes two lines.
- Desktop: the task and assistant boxes no longer show a scrollbar that cuts off their text when macOS is set to always show scroll bars.
- Desktop: in a narrow window, the task box on a project's page keeps a margin from the panel's edges.
- Desktop: in a narrow window, the harness, model and thinking picker fits inside the panel it opens from instead of being cut off at its edge.

## [0.10.20] - 2026-11-23

### Added

- Desktop: choose the flsh OpenRouter stream idle timeout in coding-agent settings. The default is 300 seconds.
- Desktop: Search the current file and enable Vim keybindings in the editor.
- Desktop: Fold functions in the file editor using gutter controls or keyboard shortcuts.
- Desktop: Use completion, automatic bracket closing, and multiple cursors in the file editor.

### Fixed

- Desktop: the chat stays full height after pressing Escape twice in a detached assistant window.
- Desktop: external links in HTML previews open in the system browser.
- Desktop: Saving a file preserves its Windows line endings.
- Desktop: agents receive saved instructions in the system prompt. An adapter that cannot apply them reports an error before the chat starts.

### Performance

- Desktop: reduce repeated checks for finished session moves while keeping progress and later moves visible.

## [0.10.19] - 2026-11-23

### Added

- Desktop: Restores open project and chat windows with their layouts and virtual desktops after a restart or update on macOS, Windows and X11. Wayland placement remains controlled by the compositor.
- Desktop: selected skills from Git libraries update automatically each day.

### Changed

- Desktop: usage has separate date, project, spend and token columns. Session totals with an unknown daily split appear in a historical table.
- Desktop: Gemini CLI is no longer offered as a harness. Antigravity replaces
  it in agent pickers, Settings and the sign-in list.

### Fixed

- Desktop: resuming an older flsh conversation no longer counts its lifetime cost as spending today.
- Desktop: Cmd+K ranks projects with more path matches higher.

### Performance

- Desktop: Session actions, prompts, model switches, cleanup, terminal resizing, device power, and settings hand off in the background instead of stalling the UI on slow or unstable node connections.
- Desktop: Project sync, Git status, session lists, pickers, devices, reviews, usage, and tool updates serve cached state immediately and isolate slow nodes so healthy machines never wait on degraded links.

## [0.10.18] - 2026-11-23

### Added

- Desktop: select and install root and nested skills independently in Settings > Skills.
- Desktop: Settings, Agent accounts has an Antigravity card that shows
  whether each machine is signed in, with a Sign in button for a machine
  that is not. The sheet shows Google's sign-in link and takes the code the
  page gives. Antigravity allows one minute for it.
- Desktop: A machine's Sign-ins list offers Sign in for Antigravity too.
- Desktop: the machine's MCP servers editor offers Antigravity beside
  Codex and Claude Code, with add, edit, disable, remove and copy for its
  stdio and Streamable HTTP servers.
- Desktop: `Cmd+K` includes quick actions for new sessions, projects, terminals, machines, keyboard shortcuts, problem reporting, and settings including appearance, instructions, and plugins.
- Desktop: Right-clicking another coding agent session while viewing a session offers `Open beside` to follow, steer, and answer it in a split dock tab.

### Changed

- Desktop: Home's Review deck displays checkout line additions and deletions, a Close button, and an inline quick-reply field to message idle sessions.
- Desktop: Light skins (`Light`, `Solarized`, `macOS`, `Warm`) use crisp light card fills, subtle top-edge highlights, and softer shadows.
- Desktop: Session headers arrange in a single aligned row without overlapping tools when resized.
- Desktop: the usage pill now shows Antigravity's and OpenRouter's icons instead of text labels.

### Fixed

- Desktop: moving the assistant panel to the bottom keeps Changes there when opening a project session.
- Desktop: assistant usage includes billed OpenRouter costs across models and keeps each machine's share when a chat moves.
- Desktop: after Esc shows all chats on macOS, the window's buttons no longer
  cover the page title.
- Desktop: open HTML previews refresh after an agent edits the file, even when its size stays the same.

### Performance

- Desktop: Faster window startup, smoother conversation scrolling, and GPU-accelerated living paintings.

## [0.10.17] - 2026-11-22

### Changed

- Desktop: In an assistant chat with threads, Esc shows all your chats in
  place of the thread list, and Esc again brings the sidebar back beside
  them.
- Desktop: Cmd+E beside a chat's threads or a project's sessions opens the
  sidebar on top of that list instead of swapping them. Cmd+E, Esc, a click
  beside it or opening another page closes it.

## [0.10.16] - 2026-11-22

### Added

- Desktop: a thread started as a persona shows the persona's name, with its
  colour dot, on its row in the chat's thread list and in the thread's header.

### Fixed

- Desktop: install a skill from a library with a top-level `SKILL.md`.

## [0.10.15] - 2026-11-22

### Added

- Desktop: an assistant chat's menu has a Skills item that picks library skills
  for the chat, the same way a project's Skills sheet does. Settings > Skills
  shows how many chats each library is installed in.

### Fixed

- Desktop: Claude conversations retain their scrollback after compaction and restart. Expand a compaction summary to read it.
- Desktop: a thread that an assistant chat starts shows in the chat's thread
  list right away instead of minutes later.
- Desktop: a file link in an assistant chat opens in the pane beside the
  chat instead of covering the window. HTML pages and Markdown files open
  in their preview.

## [0.10.14] - 2026-11-22

### Changed

- Desktop: The agent chat scroll-to-bottom button is larger, easier to see, and clearly labeled.

## [0.10.13] - 2026-11-22

### Added

- Desktop: moving an Assistant chat shows its files as they copy, such as
  "Files 120 MB of 551 MB · 22%". The speed appears once the copy has run
  for over five seconds. The progress bar along the top of the message field
  fills with the step that copies.
- Desktop: assistant chats and their threads get an Agents tab in the
  right pane. It lists the custom agents defined in the chat's folder,
  such as .claude/agents, and appears once the folder has some. Click
  an agent to see its prompt.
- Desktop: an assistant chat can open in a window of its own, with its
  threads listed beside it. Use ⇧⌘N in the chat, or "Open in New
  Window" in a chat's right-click menu. In that window, ⌘N asks the
  chat for a new thread.
- Desktop: the Assistant's chats list shows when a thread in a chat
  needs you and how many of its threads are working. A chat moves up
  the list while one of its threads is active, and the Assistant link
  in the sidebar counts the chats waiting on an answer.
- Desktop: a thread's notification in the bell names the chat it
  belongs to.

### Changed

- Desktop: ⇧⌘N in a chat opens it in its own window instead of starting
  a new thread. ⌘N starts a new thread while a chat's threads are
  listed beside it, and a new chat while the chats list shows.

### Fixed

- Desktop: a move's steps no longer stay on "Waiting for" the target while
  the move goes on. The window asks the target how far it is every few
  seconds while the move shows.
- Desktop: a session or Assistant chat you renamed keeps its name after it
  moves to another machine, instead of showing the agent's own title again.
- Desktop: clicking an HTML or Markdown file in the Files tab, or
  pressing Space on it, opens its preview instead of its source. The
  Source button still shows the code.
- Desktop: in an assistant chat on a laptop-width window, the right pane
  (Files, and a file opened from it) opens beside the conversation
  instead of covering it. The chats list steps aside while the pane
  shows and comes back when the pane is hidden.
- Desktop: notifications for a renamed coding agent session or assistant
  chat now show the name you gave it, not the title it had before.

## [0.10.12] - 2026-11-22

### Added

- Desktop: on the Assistant page, a chat's threads are listed under it. Each
  shows its name, whether it is working or needs you, and what it last said
  or asks. Click a thread to open it and write to it there.
- Desktop: "New thread" under the open chat, and in an assistant chat's menu,
  asks the chat's agent to start a thread for whatever you type.
- Desktop: the Assistant's list keeps a chat and its threads after they
  ended, also once their machine's Beep restarted. A chat's ended threads
  sit under an "Ended" fold of its own.
- Desktop: the machine name in an Assistant chat's header opens the "Move to"
  menu, as it does for a project session. Picking a machine moves the whole
  chat: its threads and its folder's files go with it.
- Desktop: the "Move to" menu lists only machines whose beep-node is new
  enough and that have every harness the chat and its threads use. A machine
  missing one says which.
- Desktop: a chat with a thread on a third machine cannot move, and says so.
- Desktop: an assistant chat's move steps above the composer say "Files"
  instead of "checkouts".
- Desktop: Cancel on a move takes back the chat's move, along with its
  threads' moves.
- Desktop: flsh sessions can be moved too.
- Desktop: an assistant chat with threads replaces the sidebar with its own
  list, like a project's sessions. It has a way back to the chats, "New
  thread", a filter, Running (the chat, then its running threads with what
  each is doing or last said) and Past (its ended threads). Press ⌘E to
  swap it with the sidebar. A chat without threads keeps the chats list.
- Desktop: ⌘N on the Assistant page starts a new chat, also from a chat or
  a thread.
- Desktop: ⇧⌘N starts a new thread in the open chat.
- Desktop: ⌃Tab and ⌃⇧Tab move to the next or previous running session of
  the open chat.
- Desktop: assistant chats and their threads get the same right pane that
  project sessions have: Files, file and HTML previews, Artifacts and
  terminals. There is no Changes tab, since a chat's folder is not a Git
  checkout, and no threads tab, since the list beside the chat shows them.
- Desktop: a chat's Files shows the chat's folder without the folders of its
  threads. A thread's Files shows the thread's own folder.
- Desktop: Settings > Agent accounts lists Antigravity under Coding agents,
  with its version, updates, on or off switch, binary path, launch arguments
  and env vars on each machine.

### Changed

- Desktop: repository owner and visibility now appear in separate labeled groups when creating a project.
- Desktop: Review cards on Home's Board tab have no Open button, so a click
  anywhere on the card, or Enter or Space when it is focused, opens the
  session.
- Desktop: Antigravity shows its own logo instead of the "AG" letters in
  the agent picker and wherever coding agents are listed.

### Fixed

- Desktop: a chat or session you ended and then resumed shows in the lists
  again at once, instead of staying hidden for up to a minute.
- Desktop: a new assistant chat keeps what you typed when you leave
  it before sending, and shows it again when you come back.
- Desktop: reopening a conversation now shows the latest message.

## [0.10.11] - 2026-11-22

### Added

- Desktop: Antigravity is one of the coding agents you can start, pick
  models for, and set up on each machine.
- Desktop: when a coding agent starts a web server, a line above the
  message field offers "Open :<port>" for its page. The Running tab
  lists that port first and highlighted, and the server's other ports
  appear only in the expanded row. A port the command names, such as
  `--port`, is preferred when it serves a page.
- Desktop: Usage shows Antigravity's weekly limits beside Claude Code's
  and Codex's.

### Changed

- Desktop: Removed the Close button from the Review card on the Board tab,
  so sessions are closed from their row menu instead.

### Fixed

- Desktop: a diagram in a chat reply shows once instead of twice after the reply redraws.
- Desktop: in a window narrow enough for the phone layout, a project's
  session list no longer sits stacked above the open session and squeezes
  it; the session takes the whole window again.
- Desktop: triple-clicking a prompt you sent in a chat selects just
  that prompt instead of running on into the replies after it (seen
  on macOS).
- Desktop: a server that a coding agent starts shows its port in the
  Running tab within about a second of it listening, instead of up to
  a minute later, or never when it runs on this desktop's own machine.

## [0.10.10] - 2026-11-21

### Added

- Desktop: a project's menu has "Harnesses...", which picks the coding agent
  harnesses that may run in that project on every machine. For example, allow
  only flsh and pi with zero data retention. Every harness may run by default.

### Changed

- Desktop: the per-harness "Can start" setting in Settings is removed. Limit
  harnesses per project instead.
- Desktop: Home's board opens on Review. The left side shows the idle
  session in hand: its project, its last reply, and Open, Close, Back and
  Next. Running sessions appear as cards under it. The right side lists
  every idle session, one line each with its project and time, grouped as
  Last hour, Today and Older.
- Desktop: in Review, Next and Back, or the left and right arrow keys, walk
  the idle list. A click picks a row, and a click on the row in hand opens
  it. The list sorts from the latest or the oldest and can show one
  project.
- Desktop: a Review / Lanes switch sits on the row of Home's tabs and shows
  while a session is open. Lanes is still there. Grouping by project on Home
  is gone. The Projects page keeps its active projects.
- Desktop: Home's machines strip no longer has "Add a machine". It stays in
  the sidebar and on the Machines tab.
- Desktop: the active projects on the Projects page are calmer. A project
  has no outline, rows have no lines between them, a row's second line is
  plain text, and the header shows no session count.

### Fixed

- Desktop: file paths in chat replies now show the hand cursor when they
  can be clicked to open in the side pane.
- Desktop: Text in a Markdown preview can now be selected and copied.

## [0.10.9] - 2026-11-21

### Added

- Desktop: on the Coding agents page, each model in flsh's per-model
  list can have its own quantizations (all, any precision, or a picked
  list) and its own auto-compact (same as all models, on, or off with a
  threshold). A model whose only OpenRouter provider serves another
  precision now works without loosening the list for every model.
- Desktop: pi's Coding agents page shows the OpenRouter routing settings:
  zero data retention, quantizations, provider sort, fallbacks, and
  per-model quantizations. flsh and pi share them.
- Desktop: when a coding agent's turn fails, its error card offers
  "Retry with another model". The model picker opens, and the model you
  pick runs the failed prompt again, without retyping it.

### Changed

- macOS: the download is now two disk images, one for Apple silicon and
  one for Intel Macs, instead of one universal app, each about half its size.
- macOS: installed apps update to the build for their own Mac
  automatically.

### Fixed

- Desktop: opening or stopping a subagent whose session is no longer on
  its machine now says so under the log, instead of doing nothing.

## [0.10.8] - 2026-11-20

### Added

- Desktop: Home's PRs and issues tab can be kept to This week or to one day. The
  arrows step back and forward a day at a time.

### Changed

- Desktop: Home's PRs and issues tab shows one card per repository in place of
  the two long columns: the repository's name, its counts and its button, then
  its pull requests and issues. A card with many items scrolls them. The sort
  menu orders the cards and the rows in them.
- Desktop: The Projects page lists the projects with open sessions on top, a row
  per session with where it runs and what it does. A project's own card no
  longer lists its sessions.
- Desktop: The rows of Home's Projects view no longer have a Close button, so
  each row's time stays in place.

## [0.10.7] - 2026-11-20

### Added

- Desktop: Settings, then Agent accounts, lets Claude Code and Codex
  each keep more than one account, such as Work and Personal, under
  Other accounts in the agent's card. Add one by signing in or pasting
  a Claude Code token or a ChatGPT personal access token.
- Desktop: the new agent form and the project bar both show an "as
  <account>" choice when the agent has named accounts. The pick is
  kept per project.
- Desktop: a start on a named account is refused on a machine whose
  beep-node is too old, instead of running on another account.
- Desktop: plan usage shows each named account's limits apart, under
  its name.
- Desktop: session cards on Home and in projects name the account a
  session runs as.
- Desktop: each coding agent harness in Settings has a "Can start" row of
  chips for the harnesses its sessions may start as other agents. For
  example, flsh may start only flsh and pi. With all chips on, a session
  may start any harness, which is the default. The setting applies on
  every machine.
- Desktop: a project's card on the Projects page lists its running
  sessions, up to three. Each shows its title, its state and what it
  does now: the current step while it works, or the first line of its
  last reply once it stops. Open a row to open its session.
- Desktop: Home's Board tab has a Lanes / Projects switch. Projects
  lists each project that has open sessions, with a row per session
  saying where it runs and what it does. The choice is remembered.

### Changed

- Desktop: after a machine's connection drops, the app tries to reach it
  again every 5 seconds instead of waiting longer and longer between
  tries. Each try gives up after 10 seconds. Terminals, file tabs and
  session views keep trying while they are on screen instead of giving
  up after a few tries.
- Desktop: project cards on the Projects page are a little wider, so
  session titles fit.

### Fixed

- Desktop: a file tab in a coding agent session's side pane that was
  open when the machine's link dropped now loads again once the machine
  is reachable, instead of staying on "connection lost". A text tab also
  no longer jumps to another copy of the file after a dropped link.
- Desktop: a question's answers are left aligned when they wrap,
  instead of centered.
- Desktop: a question that offers choices, not allow or deny, has a
  Cancel row (Cmd+.) that stops the turn. Before, nothing could cancel
  it.
- Desktop: the first choice of such a question is no longer shown red
  as a deny, and Cmd+D no longer picks it.
- Desktop: the tool call a question holds shows as waiting for you,
  not as running with a ticking clock.

## [0.10.6] - 2026-11-20

### Added

- Desktop: Settings, then the Codex harness page, has an "ACP adapter" or
  "Direct" choice, the same as Claude Code. New Codex sessions use the
  chosen driver.
- Desktop: the macOS app runs on Intel Macs as well as Apple silicon
  Macs. The download is one universal app, and installed apps keep
  updating as before.

### Changed

- Desktop: Claude Code and Codex on the Direct driver, and coding
  agents inside WSL, now run inside the app's node. The app no longer
  starts another copy of its own executable for each session.

### Fixed

- Desktop: the model and thinking of a running pi session can be switched
  from the pill by the message field.
- Desktop: after beep-node restarted and resumed an agent session, a new
  prompt's "Working" timer counted from an old turn and could show
  "Working 12h". It now counts from the new prompt.

## [0.10.5] - 2026-11-19

### Added

- Desktop: Discuss with agent prefills an editable prompt about an issue or pull request.
- Desktop: Cmd+L (Ctrl+L on Windows) opens the Assistant page with the composer ready to type. The shortcut is listed in the Keyboard shortcuts sheet.

### Changed

- Desktop: Continue opens the session that created a pull request when Beep can find it. Otherwise, Work on PR starts a new session.

### Fixed

- Desktop: resumed Codex sessions use the selected model from the start, avoiding a misleading model mismatch warning.
- Desktop: a file linked in an Assistant chat opens over the window, read from the assistant's machine. Pictures show, audio and video play, and text files show their text. An Open button opens any file in this computer's app, which is also how PDFs and 3D models open.
- Desktop: in a session's message field, the placeholder and typed text,
  the attach button, the mode pill, the model pill and the stop button
  now sit on one centred line. The mode and model pills are smaller and
  narrower, and a long model name ends in an ellipsis, leaving more room
  to type.

## [0.10.4] - 2026-11-19

### Added

- Desktop: View and edit user MCP entries in Codex and Claude Code settings, and copy them to the same harness on other machines.

### Changed

- Desktop: Beep's bundled GitHub and Forgejo agent skills now live in Beep's
  own skills folder, `~/.config/beep/skills` (or
  `$XDG_CONFIG_HOME/beep/skills`; the config folder on Windows), instead of
  the folders Claude Code, Codex and Hermes use. Agents Beep starts learn
  about every skill in that folder, so skills you add there reach only
  Beep's agents.

### Fixed

- Desktop: shows the Codex question above its answer choices.
- Desktop: on networks that block DNS over TLS, lookups now use encrypted
  DNS over HTTPS instead of the network's own DNS. A name that public DNS
  says does not exist is no longer answered by the network's DNS, which some
  hotel and filtering networks rewrite.
- Desktop: conversations load earlier messages automatically, including when recent messages fit on screen.

## [0.10.3] - 2026-11-19

### Fixed

- Desktop: copy each prompt's final reply without covering its timestamp.
- Desktop: session spend updates during a turn and includes known costs when some calls have no price.

## [0.10.2] - 2026-11-18

### Added

- Desktop: the Sync gitignored sheet reads the project's .gitignore and
  suggests the plain folders it lists, as the text box placeholder and as
  chips you click to add or remove a folder. A long list shows eight chips
  and a "more" chip. Ignored files it lists are named on one line, since
  only folders sync for now.
- Desktop: in the notifications pane (Cmd+B), pressing X dismisses the
  focused notification and moves focus to the next one.
- Desktop: the Settings sheet (cmd+,) has a search field at the top of
  its left pane, focused as soon as the sheet opens. It finds pages,
  subpages such as a coding agent or a machine, and single settings such
  as Developer mode; Enter opens the first match and a setting is
  scrolled to and marked.

### Changed

- Desktop: the Reply button on Home's Issues and Pull requests lists is
  now a three-dot menu. Issues offer "Fix with agent" (opens the project
  with its agent bar ready to work the issue in a new worktree), "Reply
  with agent" and "Open project". Pull requests offer "Reply with agent"
  and "Open project", which opens the project at its pull requests.
- Desktop: agent PR reviews run in the existing checkout without creating a worktree.

### Fixed

- Desktop: Cmd+K finds and opens conversations across your history.
- Desktop: Open on GitHub and Open on Forgejo now open the pull request in your browser.
- Desktop: Files and Changes follow the active worktree and show the correct branch in the picker.

## [0.10.1] - 2026-11-18

### Added

- Desktop: the project menu has "Sync rules…" under "Sync on all
  machines". It opens a sheet to write rules in .gitignore syntax.
  Every machine applies them to the project's sync, so no
  .beepignore needs to be committed.

### Changed

- Desktop: downloads are available from GitHub Releases.
- Desktop: deleting a session that is open in a terminal or IDE first ends the process holding it, asking it to exit and stopping it after 5 seconds. A session with no known process still refuses to delete.
- Desktop: project issue numbers appear beneath the title with the issue details.

### Fixed

- Desktop: earlier assistant chats stay listed on the Assistant page
  after they end or the machine's node restarts. Before, only chats
  still running showed there.
- Desktop: with Claude Code on the Direct driver, opening Beep no longer starts copies of itself over and over; the Claude Code bridge runs without starting the app.
- Desktop: the palette finds a project by its displayed name, not only its id.
- Desktop: a project's sessions list now shows in the left bar in small windows too, not only above 1200px.

## [0.10.0] - 2026-11-17

### Added

- Desktop: a file that is slow to open in the side pane (video, audio,
  PDF or 3D model) or to upload to a machine with a message shows its
  transfer speed, such as 4.2 MB/s. The speed appears only for transfers
  that have run more than 5 seconds and are under 80% done.
- Desktop: an issue's page shows the agent session working on it. While
  it runs, the button reads "Working" and the agent's name and opens its
  chat. After it finishes, the button reads "Open session". Both replace
  "Work on it".
- Desktop: a failed Actions run shows its agent session the same way. The
  button reads "Fixing" and the agent's name while it runs, then "Open
  session" when it finishes. Both replace "Fix this".
- Desktop: sessions started from "Work on it" are named "Issue #N", and
  sessions started from "Fix this" are named "Fix run #N
  (workflow)", as PR reviews are named "Review #N". The names cannot be
  changed, so a session can be found from any machine. Sessions started
  on an issue before this update get their name when the issue is opened.
- Desktop: in an agent session's Running tab, each command's ports are
  listed on their own line under the command instead of beside the Stop
  button. A port that a command on another machine listens on is now a
  link. Clicking it forwards that port to this computer over the mesh, on
  the same port number when it is free, and opens it in the browser.
  Before, only ports on this computer could be opened.
- Desktop: a port forward from another machine stays open until the app
  quits.
- Desktop: On a project's Issues list and Overview, a GitHub issue
  with a linked pull request that closes it shows a yellow "PR" tag.
  Forgejo issues do not show the tag.
- Desktop: In a window at least 1200px wide, opening a session that
  belongs to a project shows that project's sessions in the sidebar's
  place: a filter, the running sessions with what each is doing under
  its title (or, while it is idle, the first line of its last reply),
  then the latest past ones, one line each. Running sessions stay in
  the order they started, newest first, so they do not reshuffle as
  agents work. The reply line shows its bold, italics and code as
  notifications do. The panel ends on the project's own painting (each
  project gets one of the app's paintings) with a card of its open pull
  requests and issues, Actions and machines, each opening that part of
  the project page with the sessions still beside it. The project name goes back to the
  project page. The sidebar button swaps between the sidebar and the
  sessions with a short slide, and the conversation stays where it
  is. Cmd+E (Ctrl+E on Windows and Linux) swaps between the sidebar
  and the project's sessions, and elsewhere folds the sidebar. A
  project window shows its sessions in the sidebar's place on every
  page.
- Desktop: Settings > Instructions lists suggested rules beside the
  editor. Add puts one at the end of your instructions to review and
  save. The first one, Linear git history, asks agents to rebase a
  feature branch and merge it with a fast-forward, with no merge
  commits.
- Desktop: in a session that belongs to a project, Ctrl+Tab opens
  the next running session of that project, in the order the
  sessions panel lists them, wrapping from the last back to the
  first. Ctrl+Shift+Tab opens the previous one. Ctrl+Tab in the
  session's side pane still switches that pane's tabs.
- Desktop: a GitHub issue's detail pane on a project's page lists the
  pull requests that will close it or mention it. Clicking one opens it
  in the app when it is one of the project's open pull requests,
  otherwise on GitHub.
- Desktop: Cmd+K finds sessions by what the agent replied, not only by
  your prompts. A copy of each machine's index updates live, so results
  come at once, and sessions on offline machines are still found.
- Desktop: view supported device video using native H.264 capture, with JPEG fallback.
- Desktop: Running sessions in a project's sessions panel show the
  lines added and removed in their checkout, in green and red under
  their time. The node counts them as each turn ends, as the Changes
  tab does. It needs an updated beep-node.
- Desktop: Home's open pull requests list and Home's board cards offer
  Continue on your own pull requests.
- Desktop: while a project copies to a machine, a large file that is still
  arriving shows its name and progress, such as "pack-1a2b.pack: 12 MB of
  40 MB". It appears under the copy line in the Add to machine sheet and in
  the machine's row on the project's sync page.
- Desktop: Codex fast mode and all available thinking levels.
- Desktop: an agent reply in a coding agent session has a Copy button
  that copies the reply's text. The button appears once the reply has
  finished.
- Desktop: double-click the code of a text file in the file pane to edit
  it, with the cursor placed where you clicked.
- Desktop: The Assistant page's chats list can be resized by dragging its
  right edge or with the arrow keys on it. A double-click resets it, and the
  width is kept.
- Desktop: Shift+Tab in the assistant's composer, and in the new agent form,
  moves to the next machine.
- Desktop: when a newer beep-node release is out and online machines run
  an older one, the desktop shows a notification naming those machines.
  It appears once per release each time the app starts.
- Desktop (macOS): the notification has an Update button that updates
  every machine behind the new release. Each machine restarts once its
  agent sessions are idle. Clicking the notification opens Machines in
  Settings.
- Desktop: the main window and project windows reopen where you left
  them, at the same position and size, and come back maximized if they
  were maximized. A window last seen on a screen that is no longer
  attached opens in the default place instead.
- Desktop: a middle mouse click on a closable tab in the right panel
  (files, terminals, previews) closes it.
- Desktop: the PRs and issues tab on Home has a sort menu. Default keeps
  them by repository; Recent activity and Newest list open pull requests
  and issues from all repositories in one run, each row naming its
  repository. Your choice is remembered.
- Desktop: plugins can be skins. A plugin with the new "Restyle the app"
  permission sets colours, fonts, corners, glass and paintings, with no code
  needed, and shows in Settings, Appearance to pick like any other skin.
- Desktop: two skins ship as plugins, Dot matrix and Hardware dark. They
  start off. Turn one on in Settings, Plugins, then pick it in Appearance.
- Desktop: text files in the side pane's file tabs can now be edited.
  Edit opens an editor with syntax highlighting. Save or Cmd/Ctrl+S
  writes the file back to the machine. If the file changed since it was
  opened, saving stops and offers Reload or Overwrite. Closing the tab or
  pressing Done with unsaved changes asks first.
- Desktop: the back and forward buttons on a mouse go one page back or
  forward, the same as Cmd+[ and Cmd+].
- Desktop: an Assistant page under Home in the sidebar lists the assistant's
  chats, running ones first and ended ones under a fold, beside the open
  chat. The assistant is an agent chat with no project that runs in a
  machine's home folder. New chat, or Cmd+N on Home, opens its composer.
- Desktop: Settings has an Assistant page to pick the assistant's harness,
  model and thinking level. The choice applies to every desktop and phone.
- Desktop: with no projects yet, Home shows the assistant with an "Import my
  projects" suggestion and the projects found on your machines, each with
  Import.
- Desktop: the assistant starts on this computer when it offers the harness,
  unless you pick a machine. Other machines need a newer beep-node.
- Desktop: the start screen and empty pages, such as Home with nothing
  running, show one of four paintings that changes each day: the sunrise,
  haystacks at dusk, the lily pond with its footbridge, or Parliament at
  sunset. The first run keeps the sunrise.
- Desktop: a Developer mode switch in Settings > General. With it on, the
  desktop updates to every release, patch releases included.
- Desktop: when a session moves to another machine and its project folder
  on the source machine has uncommitted edits, the move steps above the
  composer say so. Those edits stay on the source machine. Only the session
  and the worktrees it made move.
- Desktop: a step of a session's plan shows how many subagents ran under it
  and opens to their rows; a row opens the subagent.
- Desktop: Settings under Agent accounts, Claude Code, has a new Harness
  choice: "ACP adapter" (the default) or "Direct". The choice is saved for
  every machine, and new sessions use it. The Binary path and Launch
  arguments settings apply to the ACP adapter only; Direct runs the claude
  CLI found on the machine.
- Desktop: rename any machine from its menu in the sidebar, on Home or in
  a project's machines, or with Rename on its settings page. Every desktop,
  phone and node in the mesh shows the new name.
- Desktop: a Markdown file in the right panel has a Preview button that shows it rendered, like HTML files, and Source returns to the text.
- Desktop: in the Cmd+K search palette, the right arrow on a selected
  project lists its recent coding agent sessions. Enter opens the
  selected one, and the left arrow goes back to the search results.
- Desktop: the sidebar shows a "Star on GitHub" row above the footer.
  Clicking it opens the repository in your browser. The cross on the row
  hides it for good. Nothing is starred automatically.
- Desktop: coding agents can rig a textured humanoid 3D model so it can be animated, optionally remeshing it and setting its height. The `model_3d` tool's new `rig` action needs a saved Meshy API key, takes a GLB from the session folder or an HTTP(S) URL, and saves the rigged GLB in the session's working folder.
- Desktop: an iPhone or iPad plugged into a Mac shows in the Device tab
  next to the simulators, once it is paired and Developer Mode is on. Its
  screen updates about every two seconds. Taps, typing and buttons are not
  available yet.
- Desktop: agents see plugged-in iPhones and iPads in the device tool and
  can take screenshots, install and launch apps, and open a URL in Safari.
- Desktop: a Windows machine lists its WSL distributions with the coding
  agents installed in each. Opening its page or the Add a machine sheet
  also looks into stopped distributions. Add a distribution as a machine
  with one button. Beep installs beep-node inside it and keeps it running,
  so agents installed in WSL can run there.
- Desktop: a WSL machine can be turned off and on again from its Windows
  machine's page or the Add a machine sheet. A turned-off one shows "Off"
  with "Turn on".
- Desktop: when a project's only checkout is on a machine where no coding
  agent can run yet, such as when the agent's adapter failed to install,
  the project bar names that machine and says why. A button opens it.
- Desktop: a machine's page shows its graphics cards with their video memory.
- Desktop: while an agent sends files to another machine with the `files`
  tool, its running row in the agent log shows how much has arrived, such as
  "412 MB of 1.2 GB". The count updates about once a second instead of only
  saying "Running".
- Desktop: coding agents on this Mac read a device's logs, list what its screen shows, install apps, open apps or links and clear an Android app's data with the `device` tool.
- Desktop: a notification says when a phone connects to a machine over adb,
  naming the phone and the machine. Phones already connected when the app
  opens do not notify.
- Desktop: Android phones show in the Device tab without Boot or Shut down
  buttons.
- Desktop: the Device tab's list updates as soon as a node reports a phone
  or emulator connecting or leaving. Otherwise it rereads every 60 seconds
  instead of every 10.
- Desktop: the harness, model and thinking picker (Shift+Cmd+M or the
  model pill) reads a search such as `opus 5 high`. Model words find the
  model across every harness on the machine, Claude Code and Codex first.
  A thinking level word such as low, medium, high, xhigh or max sets the
  thinking level, and a harness name such as `pi` keeps the search on that
  harness. Enter picks the first match at that level.

### Changed

- Desktop: node update notifications and updates follow Developer mode.
- Desktop: the Rename, Change and Update buttons on a machine's page sit at the right edge of their row.
- Desktop: default source builds provide a lean remote controller with plaintext replies. Build with the `full` Cargo feature or run `just run shared full` for local agents, terminals, sync, device and media hosting, rich text, and native integrations.
- Desktop: plugins load compiled JavaScript. TypeScript plugin authors must build before installing; shipped plugins continue to work.
- Desktop: Agents are instructed to use image content and the request when titling sessions, while preserving explicit titles.
- Desktop: lean builds download remote media in bounded chunks. Resumable media caching remains available with `blob-cache` or `full`.
- Desktop: plugin interfaces and themes remain available in lean builds. The plugin tool API requires `plugin-tools` or `full`; authors can check `beep.capabilities.tools` before registering tools. The Session Notes interface continues to work.
- Desktop: default builds include Markdown, sanitized rich text, math, media caching, and plugin tools.
- Desktop: local agents, terminals, files, native integrations, and resource profiling are available without selecting build features.
- Desktop: agents share Cargo build artifacts across linked worktrees.
- Desktop: a WSL distribution on a Windows machine is no longer added as
  a separate machine. The Windows machine runs the coding agents installed
  in its distributions itself. The machine page lists each distribution with
  the agents found in it, without add, turn on or turn off buttons. The Add
  a machine sheet no longer offers WSL.
- Desktop: Home's board fits the window under the Needs you queue, and
  each lane scrolls on its own a card at a time, so a long lane no longer
  makes the whole Home page scroll.
- Desktop: On Home, lanes share the width by how many cards they hold. A
  lane with more cards lays them out in two or three columns, and an
  empty lane keeps to its title.
- Desktop: The sidebar starts 300px wide. Dragging its edge resizes it
  and the project's sessions together, and the width is kept.
- Desktop: the search palette lists projects first, then machines and
  coding agents, before code, settings and actions.
- Desktop: coding agent notifications in the notifications pane show
  the agent's icon (Claude, Codex, and others) and read as three
  lines: "project: session title", then the agent's reply, then how
  long ago. The machine and agent names are gone from the text.
  System notifications use the same title and reply. A failed turn
  says "Failed:" and gives the reason.
- Desktop: On Home, the Working lane is never narrower than the other
  lanes. It spreads its cards over two or three columns, so what agents
  are doing right now stays readable. Ready to review takes at most two
  columns, and Done today keeps to one.
- Desktop: Closing a session opens its project's page instead of going
  back to Home or the page it was opened from. A session with no project
  still goes back.
- Desktop: On Home, the Working lane is one wide column, and Ready to
  review fills two columns of equal width. Lanes no longer take their
  width from how many cards they hold, so Working stays readable.
- Desktop: The Needs you buttons on Home share one width, and the
  "Review with agent" button now reads "Review".
- Desktop: answered agent questions disappear from notifications.
- Desktop: the pickers under Work on it, Continue, Review with agent, Fix this
  and Fix with an agent ask in steps: machine, harness, model, then
  thinking, with nothing picked for you. Picking the thinking level
  starts the agent. Other pickers keep their one-screen layout.
- Desktop: Closing a session in a project opens that project's next
  running session, in the order its sessions panel lists them. The
  project page opens only once no other session of it runs.
- Desktop: after updating, each machine indexes its sessions once in the
  background. A prompt you just sent shows in search once its session has
  been quiet for a minute or two.
- Desktop: search by text only covers sessions that ran inside a project
  imported on that machine. Sessions in other folders are still listed,
  but they are not found by what was said in them.
- Desktop: On Home, a pull request that an agent already reviewed now
  shows "Review again", and the new review goes to that same chat,
  resuming it if it has ended. While that review is still running, the
  button opens the chat.
- Desktop: A review started from Home now posts its comment-only review
  on the pull request, as a review started from the project page does,
  instead of waiting for approval.
- Desktop: Home's board is now a Needs you queue across the top, above
  three lanes: Working, Ready to review and Done today. Project rows are
  gone from Home. Each card shows its project's coloured dot and name.
- Desktop: an agent's question can be answered from Home with the buttons
  it offers, such as Allow or Deny, without opening the chat.
- Desktop: Working cards show what the agent is doing right now.
- Desktop: a Ready to review card has a Close button that ends the
  session. It shows when you point at the card.
- Desktop: the Home "Sort projects" menu and the Home Board/List choice
  under Settings > Appearance are removed. The Projects Board/List choice
  stays.
- Desktop: Continue on a pull request now goes on in the agent chat
  already on that PR's branch, resuming it if it has ended, instead of
  starting a new chat each time. While that chat is still working,
  Continue opens it.
- Desktop: only boards scroll a card at a time. Other pages, such as a project's Actions and Home's pull requests and issues, follow the trackpad freely.
- Desktop: file tabs in a session's side pane update when the file changes
  on the machine. Pictures, videos and audio keep their place and play
  state. PDFs, 3D models, text files, and HTML and Markdown previews update
  too, and a page reloads when a file it loaded, such as its stylesheet,
  changes. What shows stays until the new version is read, and a text file
  with unsaved edits is left alone. A tab out of sight is checked when it
  shows again. With an up-to-date beep-node, an edit that keeps a file's
  size is caught too.
- Desktop: the picked artifact on the Artifacts tab also updates when its
  file is changed outside the agent.
- Desktop: a new agent session opens at once when you send its first
  message from a project, Home, the assistant, Review with agent or
  Work on it. The app keeps the start on this device and starts the
  agent in the background. The session shows your message with what
  it is waiting for or doing under it.
- Desktop: if the machine is offline, the start waits and runs when
  the machine comes online, also after the app restarts. A waiting
  start is listed with your sessions and can be cancelled.
- Desktop: a start that fails shows the reason, with Retry and Cancel.
- Desktop: the Add a machine sheet shows a much shorter join ticket
  (`beepjoin3`) for nodes and other desktops. It is the same ticket the
  phone QR code holds. Joining another desktop's mesh accepts it too.
  Tickets from older desktops still work.
- Desktop: Fix this on a failed Actions run and Continue on a pull request
  now open the harness and model picker under the button, like Review with
  agent and Work on it. Picking a model starts the session right away and
  opens it.
- Desktop: The right-click menu's Fix this and Continue still fill the
  project's prompt bar, so you can edit the prompt first.
- Desktop: the Home assistant starts on the machine it last ran on, as a
  project's start form does, while that machine is online and offers the
  assistant's agent.
- Desktop: Fix with an agent on a file sync conflict and on a blocked Git
  sync now opens the harness and model picker under the button, like Fix
  this does. Picking a model starts the session right away. A sync conflict
  is fixed on the machine that has it, and a blocked Git sync starts with
  both checkouts and recent sessions in the prompt. It used to open a draft
  in the start form.
- Desktop: Review with agent on Home's pull request cards and in Home's pull
  request list also opens the picker and starts at once in the project's
  checkout. The review still waits for your approval before it posts
  anything. Reply on Home still opens the start form.
- Desktop: Add folder from Add a project, Choose folder, is now a search
  palette of the machine's folders, the same picker used to add a project
  to a machine. Type to filter or type a path. The path shows as steps you
  can click back through. Checkouts have a branch icon, and a checkout with
  an origin remote offers "Add org/repo". It works the same on this
  computer and on other machines. Machines to pick from show at the top.
- Desktop: in Import a project, Choose opens the same folder palette.
- Desktop: "Keep Syncthing file sync" shows only for a folder inside a
  Syncthing share. Sync options in Add a project show only when a machine
  runs Syncthing.
- Desktop: The three buttons at the top of Add a project no longer have a
  highlight along their top edge.
- Desktop: Settings "Save files to" and installing a plugin from a folder now use the same folder picker as the rest of the app, instead of the system dialog.
- Desktop: The new project folder fields have a Choose button that opens the same folder picker.
- Desktop: a project opened in its own window for the first time now
  opens at the size of the last project window. Before, it nearly
  filled the screen.
- Desktop: project session history now loads all available sessions.
- Desktop: Show the copy button only on the latest agent reply.
- Desktop: cards in lists (Home, project pages, the board's cells,
  notifications, the conversation switcher and Settings) scroll one card
  at a time. A short scroll moves one card, and a fast scroll moves
  several and slows into the card it ends on.
- Desktop: going to the next find match in a chat, or to the latest
  message, glides there, and the match lights up.
- Desktop: Home's section jumps and the diff's next file snap there
  quickly.
- Desktop: a machine removed from the mesh can be added back with a new
  pairing ticket. It keeps its identity instead of being refused with
  "this key was removed".
- Desktop: a project board's lanes show as many cards as fit on the page and
  scroll to the rest one card at a time, as on Home, instead of splitting into
  two columns. A lane with more cards than it shows hides the wanderer
  painting, so the cards get wider.
- Desktop: a painting keeps moving gently after it has painted in. Water and
  fields sway, and a few spots flicker in the light while the painting is on
  screen. It rests when it scrolls out of view or the window is hidden. The
  dawn haze behind the sidebar and lists stays still, and with Reduce motion
  on, the paintings stay still.
- Desktop: on a project's pull request page, Review with agent opens the
  harness and model picker under the button, as the diff's review button
  does. Pick a model and the reviewer session starts and opens right away.
  While the review runs, the button reads Reviewing with a spinner.
- Desktop: on an issue's page, Work on it opens the same picker. Pick a
  model and a session starts on a new branch for the issue and opens.
  Right-clicking a pull request or issue row still fills the project's
  prompt bar, so you can edit the prompt first.
- Desktop: a new agent session in an empty project, and the Start a
  coding agent page, now show the project name and the machine picker
  right under the headline, above the task field. The machine picker
  moved up from the row under the field. It still shows an online dot
  and opens the machine menu. Before, the project and machine were only
  small text at the top left.
- Desktop: starting a conversation from a project's empty agent view
  shows your message as the first message at once, with a line saying
  the agent is starting on its machine. If the start fails, the input
  comes back with what you typed and the reason.
- Desktop: a conversation started from the project bar, Home, the
  assistant, Review with agent or Work on it opens with your message
  already in it, not an empty log until the machine answers.
- Desktop: on Home, each project's board lane shows two whole cards
  with one-line titles. Scrolling moves one card at a time, and at the
  first or last card the next few scroll steps are held so the page does
  not scroll by mistake.
- Desktop: messages that Claude Code sessions send each other with Claude
  Code's own messaging, and messages from Claude Code's agents, show in
  the chat as the same small "message from session" card as Beep's own.
- Desktop: buttons, chips, inputs, menus and cards take their corner rounding from the skin you pick, so rounder skins such as Classic round them too. The default Linear skin looks the same as before.
- Desktop: the assistant's message field on Home, shown on first start,
  before any projects, works like the other agent message fields. A
  paste of more than 10 lines becomes one "Pasted text" pill that you
  can preview, expand or remove. Text after a blank line is outlined as
  code.
- Desktop: the plan usage card shows when a limit resets as a
  countdown, such as "resets in 4h 12m" or "resets in 2d 8h",
  instead of a clock time or weekday.
- Desktop: the button that copies a file to the save folder is now
  called Download.
- Desktop: the Changes tab of a coding agent session no longer opens
  its changed-files list beside the chat on a wide screen. The new
  Files button in the diff toolbar fills the session with the pane,
  as Shift+Esc does, and shows the list.
- Desktop: with the Changes pane at full size, the Files button hides
  and shows the changed-files list, and the choice is remembered. In
  history and file sync diffs the list stays beside the diff on wide
  screens, and the same button hides it.
- Desktop: the project lanes on Home scroll without showing a scrollbar.
- Desktop: a project page's board always shows Needs you, Working, Ready
  to review and Done today, so the lanes keep their place when a session
  finishes or asks something. The painting beside the lanes shows only in
  a wide window and no longer comes and goes with the cards.
- Desktop: Home sorts its project rows once after the app starts and keeps
  that order, also when you leave Home and come back, so a session
  finishing, asking or working no longer moves the rows; picking another
  sort sorts them again.
- Desktop: when a session window is too narrow for the chat beside the right
  pane, the pane folds to a strip at the right edge instead of squeezing the
  chat. The sidebar stays open.
- Desktop: the folded strip lists every tab (Files, Changes, Subagents, Device
  and others), not only Files. Picking one opens the pane over the chat; the
  fold button goes back to the chat.
- Desktop: widening the window again puts the pane back beside the chat as it
  was.
- Desktop: dimmed text, such as the board lane headers "Needs you" and
  "Working" and secondary details, is brighter and meets the WCAG 4.5:1
  contrast minimum.
- Desktop: in Add a project, clicking anywhere on a found project's row
  selects or deselects it, not only its checkbox.
- Desktop: without Developer mode, the desktop updates only to minor releases
  (such as 0.13.0) and skips patch releases.
- Desktop: while a session moves, the steps show "Waiting for checkouts
  to match" with both commits, then each worktree as it comes over
  ("Worktree name 1 of 2").
- Desktop: a project's board always shows the Working, Ready to
  review and Done today lanes, even when one is empty. Needs you
  appears only while something waits on you.
- Desktop: adding a project to a machine where the chosen folder sits
  inside a Syncthing shared folder no longer fails. Beep adds the folder
  to Syncthing's ignore list on that machine and syncs it itself. The
  folder picker says so before you add. Projects set to use Syncthing are
  still refused.
- Desktop: on a project's board, sessions that ended today are a short list
  of one-line rows with an All button, instead of full-size cards.
- Desktop: the board's lanes sit together with no blank slot for an empty
  lane. In a wide window cards keep their size and a full lane takes a
  second column.
- Desktop: cards on a project's board leave out the project's name, and a
  working card says what the agent is doing.
- Desktop: a project board with little on it shows a painting of a wanderer
  above a sea of fog beside its lanes.
- Desktop: below 1100px wide, the window folds the sidebar to its icon rail
  and opens it again when the window is wider. The toggle still works.
- Desktop: when a session is too narrow to show its pane beside the
  conversation, a strip of Chat, Files, Changes and the other tabs shows one
  at a time, full width, instead of squeezing the pane under the chat.
- Desktop: a project with nothing on its board (no running agents,
  nothing done today, no sync conflicts) now shows the agent bar centered
  under the painted sunrise, with a welcome line like the new agent screen.
  It used to sit at the bottom of an empty page.
- Desktop: a session's plan is one line above the message field, with how
  many steps are done and the one that runs. It opens to the steps.
- Desktop: an agent reply's send time now sits at the top right of the
  reply instead of on its own line above it.
- Desktop: the board leaves out an empty lane instead of showing Nothing
  here.
- Desktop: with Project sync on, projects inside a Syncthing share leave
  Syncthing on every machine, including projects imported before and
  after, so only Beep sync writes them. The "Always use Beep sync over
  Syncthing" setting is gone. A project's menu can still hand it back to
  Syncthing.
- Desktop: adding a project to a machine, and Change folder, now picks the
  folder in a search palette of that machine's folders. The palette shows
  the path as steps you can go back through. A field filters the folders
  shown, or takes a typed path. Git checkouts have a branch icon. One row
  says exactly where the project will go. Up and Down move, Enter takes the
  highlighted row, Cmd+Enter (Ctrl+Enter on Windows) adds the project, and
  Backspace goes up one folder.
- Desktop: a problem report holds each machine's whole log for the chosen
  period, instead of the newest 1 MiB of each. A machine on an older node
  still gives its newest 1 MiB.
- Desktop: a problem report can be up to 95 MiB, up from 5 MiB, and a large
  one gets more time to upload.
- Desktop: the quick window takes the main window's look: monospaced details,
  a status as a dot and a word, and the same messages and message field.
- Desktop: the Beep tools for coding agents now have one tool per action, such as `list_machines`, `remote_run`, `remote_run_status`, `send_files`, `start_agent`, `device_tap`, `beep_logs`, `generate_image`, `generate_3d`, `rig_3d` and `list_models`. Each tool takes only its own arguments, so agents mix up arguments less often.
- Desktop: the `plugins` tool is removed, so coding agents can no longer list, create or call Beep plugins.
- Desktop: the log says where a failed incoming connection came from:
  its address, or its endpoint id when it came through a relay. Warnings
  for an incoming connection name its protocol and machine.
- Desktop: the phone pairing QR code holds far less data and is drawn larger,
  so a phone camera reads it at once. The copied ticket is unchanged.
- Desktop: the start form with a Project menu is gone. Chats now start
  inside a project. Cmd+N outside a project, Start an agent on Home, and
  New in the tray open Projects instead.
- Desktop: Review and Reply on pull requests and issues only show for
  repositories that are projects. Other repositories keep Import project.
- Desktop: the tray no longer opens a start form when nothing is waiting.
  It shows its empty list.
- Desktop: the model picker search matches the thinking level by its start, as
  it matches models: `opus 5.5 h` or `deepseek v4 med` shows rows such as
  `Opus 5.5 · High`. A version typed as `5.5` also finds ids written `5-5`.
- Desktop: every other block between triple line breaks in a message now goes out as code, not only the first.
- Desktop: built-in plugins start turned off on first run. Turn them on in Plugins.
- Desktop: Home's "PRs and issues" tab is now two plain lists, pull
  requests on the left and issues on the right, grouped by repository
  with one line per item. Review and Reply show on the row under the
  pointer. Each repository has a button that opens its project or
  imports it.
- Desktop: The label cloud, time filter, sort and issue picker are no
  longer on Home. They remain on the project page.

### Fixed

- Desktop: minor release notes now include changes from preceding patch releases, even without pending entries.
- Desktop: Linux installers build with complete workspace inputs.
- Desktop: fixed the macOS release build so installers can be published.
- Desktop: delegated agents and their status appear in flsh sessions when the harness reports them.
- Desktop: A session that works in another project's checkout stays
  with the project it started in, in the project sessions panel too.
- Desktop: an interrupted Git transfer no longer leaves half-written pack files
  in `.git`. A stalled transfer gives up after 30 seconds of silence instead
  of waiting five minutes. Git packs its objects together once many have
  arrived.
- Desktop: in a diff, "Review with agent" shows a reviewer that is
  still working after the window reloads or its machine's connection
  drops, so you can open it again. Before, the button went back to
  "Review with agent" and hid the running reviewer.
- Desktop: In a project's new conversation (Cmd+N from a project,
  including in a project window), Shift+Cmd+M and the model pill open the
  model picker again.
- Desktop: media viewers reject incomplete or changing remote downloads instead of displaying partial files.
- Desktop: development logs no longer contain color codes.
- Desktop: retrying a session that failed to start no longer crashes the app.
- Desktop: opening a project focuses the agent composer, and closing the model picker with Cmd+Shift+M returns focus to the input.
- Desktop: paintings no longer crash on startup when visibility tracking is unavailable.
- Desktop: answer buttons no longer squeeze question text.
- Desktop: on Wi-Fi networks that rewrite DNS answers, such as some hotel
  content filters, the app could not reach any machine. Names now resolve
  through encrypted DNS first, then the network's DNS if that fails.
- Desktop: In a project window too narrow for its sessions, the Back
  button no longer sits under the window's traffic lights.
- Desktop: in the search palette, a coding agent found by its prompt
  shows one short line from that prompt, without a "Prompt:" label,
  repeated excerpts for each search word, or attached file paths.
- Desktop: a palette row stays on one line when the search has more
  than one word, instead of wrapping onto several lines.
- Desktop: coding agent results in the search palette show the agent's
  icon, with only the project name on the right.
- Desktop: the task field on a project page keeps an unsent task for
  each project. Text typed in one project no longer follows you to the
  next one or overlaps its placeholder. Drafts survive an app restart.
- Desktop: when a machine is slow to start following a folder, the Files
  tab reads it directly instead of showing that the session subscription
  did not answer in time.
- Desktop: the time on each project card and the Recent activity order
  now agree.
- Desktop: the usage pill at the top no longer keeps Claude's plan percentages
  from an earlier run while Codex's numbers update. Claude's current numbers
  replace the old ones as soon as a machine reads them.
- Desktop: a coding agent's reply in the notifications pane renders
  its markdown (bold, italic, code) on up to two lines instead of
  showing raw marks or HTML such as `<br>`.
- Desktop: system notifications show the reply as plain text.
- Desktop: opening a session no longer narrows the conversation a
  moment after it shows. The files pane was first sized for a
  narrower window and then widened, so a conversation you already
  scrolled now keeps its place.
- Desktop: starting another app build no longer disrupts a running frontend watcher.
- Desktop: a file link in a reply from a session working in a subfolder
  of a repository now opens in the file pane when the file is in a
  worktree at the repository's top, such as `.worktrees/fix/...`.
  Before, the pane looked only in the session's folder and failed.
- Desktop: device streaming keeps video and touch input moving independently, without losing drawing acknowledgements under input load.
- Desktop: the project bar's machine menu lists an online machine that
  has a checkout and a coding agent even when it reported itself while
  the bar was off screen, such as right after the app restarts.
- Desktop: a coding agent session started in a terminal outside Beep now
  shows as ended as soon as its process exits, so you can continue it in
  Beep right away. Before, it showed as running outside Beep for up to
  three minutes.
- Desktop: the Fast mode switch in the picker under Work on it and the
  other agent buttons shows as a switch again, instead of a stray dot
  in the corner.
- Desktop (macOS): an expanded diagram or picture opened from the attachments
  window no longer puts its title under the close, minimize and zoom buttons.
- Desktop: resumed Claude sessions recover available earlier messages from native history.
- Desktop: fixing a failed workflow run that a tag started, such as a
  release, now starts the agent on a new branch from that tag. Before,
  it failed because no branch had the tag's name.
- Desktop: the spinner on the plan line above the composer is a small circle again instead of a stretched slash.
- Desktop: the rows in the machine and harness steps of the picker under
  Work on it and the other agent buttons light up when pointed at.
- Desktop: the session title in a chat header ends where its text ends, so pointing at or double-clicking the empty bar beside it no longer starts a rename.
- Desktop: with an HTML preview focused in the session pane, Cmd+1 leaves
  the zoomed pane and focuses the message field, and Cmd+W closes the tab,
  as they do elsewhere. Cmd+2 to Cmd+9 also switch tabs.
- Desktop: Pressing Esc on the Assistant page now goes back to Home.
- Desktop: scrolling the Actions page with a run open no longer gets stuck in the run's log.
- Desktop: Shift+Esc zooms the side pane again when an HTML page in the
  Artifacts tab or a file's Preview has keyboard focus. Before, clicking
  into the page swallowed the key.
- Desktop: the harness, model and thinking picker (opened from Review with
  agent, Work on it, Fix this and the model pills) and the new agent form's
  machine and folder menus now open wherever they fit in the window. Before,
  Review with agent on Home opened the picker cut off under the sidebar and
  header.
- Desktop: a Markdown file previewed in a file tab scrolls again instead of cutting off below the first screen.
- Desktop: image-only agent messages now include a clear default prompt instead of using filenames as the task.
- Desktop: SVG files open in the image viewer and can be previewed from Files.
- Desktop: a project board stops flickering during agent activity in a wide window. The painting beside the lanes no longer vanishes as they switch between wide and narrow.
- Desktop: coding agent updates grant Homebrew trust only to the installed formula when required, then retry the upgrade instead of hiding the failure behind the agent updater.
- Desktop: image attachments no longer become conversation titles when a session has no name.
- Desktop: the project board no longer loops while it measures its lanes.
  Before, the whole window could go blank after the laptop woke up
  offline or after a session view closed.
- Desktop: a conversation just started no longer drops the message you
  sent and shows "Starting the agent" for a few seconds while its
  machine sets the model and thinking level.
- Desktop: a Claude Code turn that finished normally no longer shows
  "The turn did not finish" when the same step asked for the advisor
  next to a local command. The advisor request never ran, so that turn
  now ends normally.
- Desktop: switching the project on the start screen no longer replays the
  headline's words or briefly wraps it onto two lines, so the composer under
  it stays put.
- Desktop: when a part of the window fails to draw, only that part shows
  "This part failed to draw" with a Try again button. The rest of the
  window keeps working. Before, the whole window went blank until you
  restarted the app.
- Desktop: adding a project to a machine in a new folder inside a Syncthing share works instead of failing with "the target folder does not exist".
- Desktop: project links consistently support opening a separate window.
- Desktop: show files and changes from the agent's linked Git worktree.
- Desktop: sent images retain their detail and reopen at full quality.
- Desktop: the sidebar no longer says it is connecting to paired phones, which control the mesh and are never dialed.
- Desktop: on Home, scrolling up with the pointer over a board cell
  scrolls the page again.
- Desktop: the Copy button under a quote at the end of an agent reply
  appears once the reply finishes, not while the quote is still
  streaming in.
- Desktop: the output of an opened tool call can be selected again.
- Desktop: a notification for a coding agent question that asks you to
  choose an answer no longer offers Allow and Deny. Clicking it opens the
  chat so you can pick an answer. This applies to system notifications
  and the bell.
- Desktop: answering a question in the chat marks its notification as
  answered right away, instead of leaving Allow and Deny showing for
  minutes.
- Desktop: a session that compacts its conversation shows "Compacting the
  conversation" instead of "Waiting for 1 command" when a background command
  is still running.
- Desktop: A message card in an agent session now reads "From <project>" for the sender's project, so it no longer looks like the session lives in that project.
- Desktop: in the diff viewer, pressing x folded the file at the top of
  the scroll instead of the file you clicked. Clicking anywhere in a
  file now makes it the current file, so x folds or opens that one.
- Desktop: On macOS, the text cursor in the chat composer and the new-session composer is now the same height as the text instead of taller than it.
- Desktop: selected code in the file editor is readable on light skins.
- Desktop: code in the file view can be selected.
- Desktop: syntax colours for keywords, strings and numbers are readable
  on light skins.
- Desktop: terminals now use the colours of the picked skin: background,
  text, cursor, selection and the main ANSI colours. With a light skin the
  terminal is light too instead of staying dark, and open terminals change
  at once when the skin changes.
- Desktop: A chat started from Home's composer or the Assistant page opens on
  the Assistant page beside its chats instead of the plain session view.
- Desktop: a waiting message from another session shows in the composer
  queue with the sender's name and its words, instead of a "Steer" row
  with the raw tag.
- Desktop: a project's machine card showed two Fix with an agent buttons for
  the same Git sync failure, one by the sync status and one by the Git
  controls. The card now shows one.
- Desktop: on light skins the instructions editor, reply box, version history preview and terminal screen show a light background instead of a gray one.
- Desktop: 0.9.17 did not ship for Linux because its build failed. The
  two built-in skins were missing from the Linux build. This version ships
  the Linux packages again, with the skins.
- Desktop: the plan line above the message field shows its status as a dot again instead of a long blue streak.
- Desktop: Mermaid diagrams in an agent's reply no longer flicker while
  the reply streams. A diagram draws once its block is complete and
  stays put as more text arrives.
- Desktop: a diagram no longer shows an empty strip along its right
  side.
- Desktop: the sidebar icons (Search, Home, Terminals, Settings) sit level
  with their labels instead of slightly low.
- Desktop: the notifications bell is centred in its button.
- Desktop: a board card's title is as far from the card's top edge as from
  its side.
- Desktop: screen readers and voice control announce sidebar items, project
  rows, machine rows, sort menus and the session title with the same names
  you see on screen.
- Desktop: typing `/` in the new agent field, on the start screen and in
  the project bar, lists the agent's slash commands even on a machine that
  has no running session of that coding agent. The list comes from a running
  session of the same agent on another machine.
- Desktop: a subagent card's result keeps its line breaks and sits under the
  card's title instead of running together as one block.
- Desktop: right-click menus, such as a file tab's menu in the side
  panel, stay open when the panel updates and restores its scroll
  position. They still close when you scroll with the mouse wheel or
  a touch drag.
- Desktop: on the board (Home and a project's page), a Working card with
  a long running command now cuts the command with an ellipsis at the
  card's edge instead of spilling into the next lane.
- Desktop: the sunrise behind the new task headline fades out softly
  inside the screen instead of being cut off at its left and right
  edges.
- Desktop: the dark oval behind the headline on the new task screen
  is now a soft shade that fades into the sunrise.
- Desktop: an agent that offers more than 500 models, such as flsh with several
  providers set up, shows all of them in the model picker instead of
  dropping the providers listed last.
- Desktop: on Windows and Linux the window's title bar is dark with the
  dark skins and light with the light ones, instead of always following
  the system theme.
- Desktop: a project window on macOS no longer puts the traffic-light
  buttons over the project name.
- Desktop: the project menu no longer offers "Use Beep sync instead of
  Syncthing" for a project whose folder is not in a Syncthing share on
  any machine.
- Desktop: in the model picker, a row is highlighted only when the cursor
  moves over it. A cursor resting on a row no longer highlights that row,
  and Enter picks the highlighted row.
- Desktop: an attached image opens in the viewer as the picture you
  attached, not the small preview that goes inline.
- Desktop: adding a project that uses Beep sync to a machine where the
  folder is inside a Syncthing share adds the Syncthing ignore rule there
  instead of refusing the folder.
- Desktop: with Project sync on, the import and add folder sheets show
  "Keep Syncthing file sync" off for a folder in a Syncthing share,
  matching what happens when it is left alone.
- Desktop: a project can be added to a folder that already holds its files
  without `.git`, for example files copied there by another file sync. The
  Git history arrives and the files stay as they are. Adding no longer
  stops with "has other files" or makes a nested `project/project` folder.
- Desktop: in the Import and Add a folder sheets, a long path in the folder
  browser's note no longer runs under its buttons.
- Desktop: on Linux, the AppImage no longer crashes at start. It now
  includes the tray icon library (ayatana-appindicator) instead of
  relying on the system copy. The .deb package depends on that library.
- Desktop: a session's usage in the quick window's header shows its context
  meter, tokens and cost apart instead of as one run of text.
- Desktop: in an agent session, Cmd+Shift+M opens the model picker with
  its search focused, so typing a model name picks it instead of going
  to the message field.
- Desktop: when no machine has a coding agent, Enter in the project bar and on the new agent page now says why nothing started and offers "Install a coding agent", which opens Coding agents to install Claude Code, Codex and others on a machine.
- Desktop: on Windows and Linux, errors that said "this Mac" now say "this computer". The Claude Code sign-in error also says to install it from Coding agents.
- Desktop: the sidebar section labels (Pinned, Projects, Machines) are as bright as Home and Terminals, so they stay readable over the sidebar painting.
- Desktop: a new chat in a narrow chat pane, next to the files panel,
  no longer scrolls sideways. The start screen fits the pane.
- Desktop: while a turn is stopping, the key hints under the composer keep
  their row, so Show more stays visible.
- Desktop: code blocks in chat replies no longer show a horizontal scrollbar over a one-line block.
- Desktop: a phone connected over adb to the Mac running the app shows a notification and appears in the Device tab at once, as it already did on other machines.
- Desktop: Shift+Cmd+M opens the model picker in an agent conversation, not only on the start form.
- Desktop: a finished agent turn with an unknown length no longer shows a bare "Worked" heading.
- Desktop: the model picker's search field uses the same frosted glass as the rest of the picker instead of a dark band.
- Desktop: in the search palette (Cmd+K), coding agent results found by their prompt show the matching prompt on its own line again, instead of squeezing the title, details and prompt onto one line.
- Desktop: the Linux installers (Debian and AppImage) build again; the 0.9.0 Linux build failed because the example plugins were missing from the build.

### Performance

- Desktop: Git sync between machines sends only the commits the other machine
  is missing, instead of the whole history for each new branch. A repository
  that grew to tens of gigabytes from repeated copies stays its normal size.
- Desktop: only the default branch and the branch checked out on each machine
  sync. Agent worktree branches stay on the machine that made them. A branch
  that both machines have fast-forwards while it is checked out on the
  machine that made the commits.
- Desktop: builds compile fewer dependencies.
- Desktop: source builds compile fewer dependencies while retaining device control, Git sync, and TypeScript plugins.
- Desktop: the app no longer reads every transcript over the mesh to build
  its own index, which saves data and battery.
- Desktop: source builds compile fewer dependencies while preserving app features.
- Desktop: regular builds omit resource profiling dependencies; build with the `resource-profiling` feature and enable profiling at runtime to use it.
- Desktop: opening a session loads only the newest entries that fit the
  window, plus a few more, instead of the last 200. It loads with less
  data. Older entries still load as you scroll up.
- Desktop: sessions appear faster when listing local and remote agents.
- Desktop: Git sync and the branch views keep each repository open between
  reads instead of loading its pack indexes again on every scan, which uses
  less CPU and battery.
- Desktop: a finished painting on the start screen and empty pages wakes the
  app only when it moves, ten times a second, instead of on every screen
  refresh, so it uses less battery. It looks the same.
- Desktop: The folder picker reads three levels of folders in one request, so opening a subfolder shows at once. Folders read before show at once and refresh in the background.

## [0.9.28] - 2026-11-17

### Fixed

- Desktop: Linux installers build with complete workspace inputs.

## [0.9.27] - 2026-11-17

### Changed

- Desktop: node update notifications and updates follow Developer mode.

### Fixed

- Desktop: fixed the macOS release build so installers can be published.

## [0.9.26] - 2026-11-17

### Added

- Desktop: a file that is slow to open in the side pane (video, audio,
  PDF or 3D model) or to upload to a machine with a message shows its
  transfer speed, such as 4.2 MB/s. The speed appears only for transfers
  that have run more than 5 seconds and are under 80% done.
- Desktop: an issue's page shows the agent session working on it. While
  it runs, the button reads "Working" and the agent's name and opens its
  chat. After it finishes, the button reads "Open session". Both replace
  "Work on it".
- Desktop: a failed Actions run shows its agent session the same way. The
  button reads "Fixing" and the agent's name while it runs, then "Open
  session" when it finishes. Both replace "Fix this".
- Desktop: sessions started from "Work on it" are named "Issue #N", and
  sessions started from "Fix this" are named "Fix run #N
  (workflow)", as PR reviews are named "Review #N". The names cannot be
  changed, so a session can be found from any machine. Sessions started
  on an issue before this update get their name when the issue is opened.
- Desktop: in an agent session's Running tab, each command's ports are
  listed on their own line under the command instead of beside the Stop
  button. A port that a command on another machine listens on is now a
  link. Clicking it forwards that port to this computer over the mesh, on
  the same port number when it is free, and opens it in the browser.
  Before, only ports on this computer could be opened.
- Desktop: a port forward from another machine stays open until the app
  quits.
- Desktop: On a project's Issues list and Overview, a GitHub issue
  with a linked pull request that closes it shows a yellow "PR" tag.
  Forgejo issues do not show the tag.
- Desktop: In a window at least 1200px wide, opening a session that
  belongs to a project shows that project's sessions in the sidebar's
  place: a filter, the running sessions with what each is doing under
  its title (or, while it is idle, the first line of its last reply),
  then the latest past ones, one line each. Running sessions stay in
  the order they started, newest first, so they do not reshuffle as
  agents work. The reply line shows its bold, italics and code as
  notifications do. The panel ends on the project's own painting (each
  project gets one of the app's paintings) with a card of its open pull
  requests and issues, Actions and machines, each opening that part of
  the project page with the sessions still beside it. The project name goes back to the
  project page. The sidebar button swaps between the sidebar and the
  sessions with a short slide, and the conversation stays where it
  is. Cmd+E (Ctrl+E on Windows and Linux) swaps between the sidebar
  and the project's sessions, and elsewhere folds the sidebar. A
  project window shows its sessions in the sidebar's place on every
  page.
- Desktop: Settings > Instructions lists suggested rules beside the
  editor. Add puts one at the end of your instructions to review and
  save. The first one, Linear git history, asks agents to rebase a
  feature branch and merge it with a fast-forward, with no merge
  commits.
- Desktop: in a session that belongs to a project, Ctrl+Tab opens
  the next running session of that project, in the order the
  sessions panel lists them, wrapping from the last back to the
  first. Ctrl+Shift+Tab opens the previous one. Ctrl+Tab in the
  session's side pane still switches that pane's tabs.
- Desktop: a GitHub issue's detail pane on a project's page lists the
  pull requests that will close it or mention it. Clicking one opens it
  in the app when it is one of the project's open pull requests,
  otherwise on GitHub.
- Desktop: Cmd+K finds sessions by what the agent replied, not only by
  your prompts. A copy of each machine's index updates live, so results
  come at once, and sessions on offline machines are still found.
- Desktop: view supported device video using native H.264 capture, with JPEG fallback.
- Desktop: Running sessions in a project's sessions panel show the
  lines added and removed in their checkout, in green and red under
  their time. The node counts them as each turn ends, as the Changes
  tab does. It needs an updated beep-node.

### Changed

- Desktop: the Rename, Change and Update buttons on a machine's page sit at the right edge of their row.
- Desktop: default source builds provide a lean remote controller with plaintext replies. Build with the `full` Cargo feature or run `just run shared full` for local agents, terminals, sync, device and media hosting, rich text, and native integrations.
- Desktop: plugins load compiled JavaScript. TypeScript plugin authors must build before installing; shipped plugins continue to work.
- Desktop: Agents are instructed to use image content and the request when titling sessions, while preserving explicit titles.
- Desktop: lean builds download remote media in bounded chunks. Resumable media caching remains available with `blob-cache` or `full`.
- Desktop: plugin interfaces and themes remain available in lean builds. The plugin tool API requires `plugin-tools` or `full`; authors can check `beep.capabilities.tools` before registering tools. The Session Notes interface continues to work.
- Desktop: default builds include Markdown, sanitized rich text, math, media caching, and plugin tools.
- Desktop: local agents, terminals, files, native integrations, and resource profiling are available without selecting build features.
- Desktop: agents share Cargo build artifacts across linked worktrees.
- Desktop: a WSL distribution on a Windows machine is no longer added as
  a separate machine. The Windows machine runs the coding agents installed
  in its distributions itself. The machine page lists each distribution with
  the agents found in it, without add, turn on or turn off buttons. The Add
  a machine sheet no longer offers WSL.
- Desktop: Home's board fits the window under the Needs you queue, and
  each lane scrolls on its own a card at a time, so a long lane no longer
  makes the whole Home page scroll.
- Desktop: On Home, lanes share the width by how many cards they hold. A
  lane with more cards lays them out in two or three columns, and an
  empty lane keeps to its title.
- Desktop: The sidebar starts 300px wide. Dragging its edge resizes it
  and the project's sessions together, and the width is kept.
- Desktop: the search palette lists projects first, then machines and
  coding agents, before code, settings and actions.
- Desktop: coding agent notifications in the notifications pane show
  the agent's icon (Claude, Codex, and others) and read as three
  lines: "project: session title", then the agent's reply, then how
  long ago. The machine and agent names are gone from the text.
  System notifications use the same title and reply. A failed turn
  says "Failed:" and gives the reason.
- Desktop: On Home, the Working lane is never narrower than the other
  lanes. It spreads its cards over two or three columns, so what agents
  are doing right now stays readable. Ready to review takes at most two
  columns, and Done today keeps to one.
- Desktop: Closing a session opens its project's page instead of going
  back to Home or the page it was opened from. A session with no project
  still goes back.
- Desktop: On Home, the Working lane is one wide column, and Ready to
  review fills two columns of equal width. Lanes no longer take their
  width from how many cards they hold, so Working stays readable.
- Desktop: The Needs you buttons on Home share one width, and the
  "Review with agent" button now reads "Review".
- Desktop: answered agent questions disappear from notifications.
- Desktop: the pickers under Work on it, Continue, Review with agent, Fix this
  and Fix with an agent ask in steps: machine, harness, model, then
  thinking, with nothing picked for you. Picking the thinking level
  starts the agent. Other pickers keep their one-screen layout.
- Desktop: Closing a session in a project opens that project's next
  running session, in the order its sessions panel lists them. The
  project page opens only once no other session of it runs.
- Desktop: after updating, each machine indexes its sessions once in the
  background. A prompt you just sent shows in search once its session has
  been quiet for a minute or two.
- Desktop: search by text only covers sessions that ran inside a project
  imported on that machine. Sessions in other folders are still listed,
  but they are not found by what was said in them.

### Fixed

- Desktop: delegated agents and their status appear in flsh sessions when the harness reports them.
- Desktop: A session that works in another project's checkout stays
  with the project it started in, in the project sessions panel too.
- Desktop: an interrupted Git transfer no longer leaves half-written pack files
  in `.git`. A stalled transfer gives up after 30 seconds of silence instead
  of waiting five minutes. Git packs its objects together once many have
  arrived.
- Desktop: in a diff, "Review with agent" shows a reviewer that is
  still working after the window reloads or its machine's connection
  drops, so you can open it again. Before, the button went back to
  "Review with agent" and hid the running reviewer.
- Desktop: In a project's new conversation (Cmd+N from a project,
  including in a project window), Shift+Cmd+M and the model pill open the
  model picker again.
- Desktop: media viewers reject incomplete or changing remote downloads instead of displaying partial files.
- Desktop: development logs no longer contain color codes.
- Desktop: retrying a session that failed to start no longer crashes the app.
- Desktop: opening a project focuses the agent composer, and closing the model picker with Cmd+Shift+M returns focus to the input.
- Desktop: paintings no longer crash on startup when visibility tracking is unavailable.
- Desktop: answer buttons no longer squeeze question text.
- Desktop: on Wi-Fi networks that rewrite DNS answers, such as some hotel
  content filters, the app could not reach any machine. Names now resolve
  through encrypted DNS first, then the network's DNS if that fails.
- Desktop: In a project window too narrow for its sessions, the Back
  button no longer sits under the window's traffic lights.
- Desktop: in the search palette, a coding agent found by its prompt
  shows one short line from that prompt, without a "Prompt:" label,
  repeated excerpts for each search word, or attached file paths.
- Desktop: a palette row stays on one line when the search has more
  than one word, instead of wrapping onto several lines.
- Desktop: coding agent results in the search palette show the agent's
  icon, with only the project name on the right.
- Desktop: the task field on a project page keeps an unsent task for
  each project. Text typed in one project no longer follows you to the
  next one or overlaps its placeholder. Drafts survive an app restart.
- Desktop: when a machine is slow to start following a folder, the Files
  tab reads it directly instead of showing that the session subscription
  did not answer in time.
- Desktop: the time on each project card and the Recent activity order
  now agree.
- Desktop: the usage pill at the top no longer keeps Claude's plan percentages
  from an earlier run while Codex's numbers update. Claude's current numbers
  replace the old ones as soon as a machine reads them.
- Desktop: a coding agent's reply in the notifications pane renders
  its markdown (bold, italic, code) on up to two lines instead of
  showing raw marks or HTML such as `<br>`.
- Desktop: system notifications show the reply as plain text.
- Desktop: opening a session no longer narrows the conversation a
  moment after it shows. The files pane was first sized for a
  narrower window and then widened, so a conversation you already
  scrolled now keeps its place.
- Desktop: starting another app build no longer disrupts a running frontend watcher.
- Desktop: a file link in a reply from a session working in a subfolder
  of a repository now opens in the file pane when the file is in a
  worktree at the repository's top, such as `.worktrees/fix/...`.
  Before, the pane looked only in the session's folder and failed.
- Desktop: device streaming keeps video and touch input moving independently, without losing drawing acknowledgements under input load.
- Desktop: the project bar's machine menu lists an online machine that
  has a checkout and a coding agent even when it reported itself while
  the bar was off screen, such as right after the app restarts.
- Desktop: a coding agent session started in a terminal outside Beep now
  shows as ended as soon as its process exits, so you can continue it in
  Beep right away. Before, it showed as running outside Beep for up to
  three minutes.
- Desktop: the Fast mode switch in the picker under Work on it and the
  other agent buttons shows as a switch again, instead of a stray dot
  in the corner.
- Desktop (macOS): an expanded diagram or picture opened from the attachments
  window no longer puts its title under the close, minimize and zoom buttons.
- Desktop: resumed Claude sessions recover available earlier messages from native history.
- Desktop: fixing a failed workflow run that a tag started, such as a
  release, now starts the agent on a new branch from that tag. Before,
  it failed because no branch had the tag's name.
- Desktop: the spinner on the plan line above the composer is a small circle again instead of a stretched slash.
- Desktop: the rows in the machine and harness steps of the picker under
  Work on it and the other agent buttons light up when pointed at.
- Desktop: the session title in a chat header ends where its text ends, so pointing at or double-clicking the empty bar beside it no longer starts a rename.
- Desktop: with an HTML preview focused in the session pane, Cmd+1 leaves
  the zoomed pane and focuses the message field, and Cmd+W closes the tab,
  as they do elsewhere. Cmd+2 to Cmd+9 also switch tabs.

### Performance

- Desktop: Git sync between machines sends only the commits the other machine
  is missing, instead of the whole history for each new branch. A repository
  that grew to tens of gigabytes from repeated copies stays its normal size.
- Desktop: only the default branch and the branch checked out on each machine
  sync. Agent worktree branches stay on the machine that made them. A branch
  that both machines have fast-forwards while it is checked out on the
  machine that made the commits.
- Desktop: builds compile fewer dependencies.
- Desktop: source builds compile fewer dependencies while retaining device control, Git sync, and TypeScript plugins.
- Desktop: the app no longer reads every transcript over the mesh to build
  its own index, which saves data and battery.
- Desktop: source builds compile fewer dependencies while preserving app features.
- Desktop: regular builds omit resource profiling dependencies; build with the `resource-profiling` feature and enable profiling at runtime to use it.
- Desktop: opening a session loads only the newest entries that fit the
  window, plus a few more, instead of the last 200. It loads with less
  data. Older entries still load as you scroll up.
- Desktop: sessions appear faster when listing local and remote agents.
- Desktop: Git sync and the branch views keep each repository open between
  reads instead of loading its pack indexes again on every scan, which uses
  less CPU and battery.

## [0.9.25] - 2026-11-16

### Added

- Desktop: Home's open pull requests list and Home's board cards offer
  Continue on your own pull requests.

### Changed

- Desktop: On Home, a pull request that an agent already reviewed now
  shows "Review again", and the new review goes to that same chat,
  resuming it if it has ended. While that review is still running, the
  button opens the chat.
- Desktop: A review started from Home now posts its comment-only review
  on the pull request, as a review started from the project page does,
  instead of waiting for approval.
- Desktop: Home's board is now a Needs you queue across the top, above
  three lanes: Working, Ready to review and Done today. Project rows are
  gone from Home. Each card shows its project's coloured dot and name.
- Desktop: an agent's question can be answered from Home with the buttons
  it offers, such as Allow or Deny, without opening the chat.
- Desktop: Working cards show what the agent is doing right now.
- Desktop: a Ready to review card has a Close button that ends the
  session. It shows when you point at the card.
- Desktop: the Home "Sort projects" menu and the Home Board/List choice
  under Settings > Appearance are removed. The Projects Board/List choice
  stays.
- Desktop: Continue on a pull request now goes on in the agent chat
  already on that PR's branch, resuming it if it has ended, instead of
  starting a new chat each time. While that chat is still working,
  Continue opens it.
- Desktop: only boards scroll a card at a time. Other pages, such as a project's Actions and Home's pull requests and issues, follow the trackpad freely.

### Fixed

- Desktop: Pressing Esc on the Assistant page now goes back to Home.
- Desktop: scrolling the Actions page with a run open no longer gets stuck in the run's log.

## [0.9.24] - 2026-11-16

### Added

- Desktop: while a project copies to a machine, a large file that is still
  arriving shows its name and progress, such as "pack-1a2b.pack: 12 MB of
  40 MB". It appears under the copy line in the Add to machine sheet and in
  the machine's row on the project's sync page.

### Changed

- Desktop: file tabs in a session's side pane update when the file changes
  on the machine. Pictures, videos and audio keep their place and play
  state. PDFs, 3D models, text files, and HTML and Markdown previews update
  too, and a page reloads when a file it loaded, such as its stylesheet,
  changes. What shows stays until the new version is read, and a text file
  with unsaved edits is left alone. A tab out of sight is checked when it
  shows again. With an up-to-date beep-node, an edit that keeps a file's
  size is caught too.
- Desktop: the picked artifact on the Artifacts tab also updates when its
  file is changed outside the agent.

### Fixed

- Desktop: Shift+Esc zooms the side pane again when an HTML page in the
  Artifacts tab or a file's Preview has keyboard focus. Before, clicking
  into the page swallowed the key.
- Desktop: the harness, model and thinking picker (opened from Review with
  agent, Work on it, Fix this and the model pills) and the new agent form's
  machine and folder menus now open wherever they fit in the window. Before,
  Review with agent on Home opened the picker cut off under the sidebar and
  header.
- Desktop: a Markdown file previewed in a file tab scrolls again instead of cutting off below the first screen.
- Desktop: image-only agent messages now include a clear default prompt instead of using filenames as the task.

## [0.9.23] - 2026-11-16

### Fixed

- Desktop: SVG files open in the image viewer and can be previewed from Files.
- Desktop: a project board stops flickering during agent activity in a wide window. The painting beside the lanes no longer vanishes as they switch between wide and narrow.
- Desktop: coding agent updates grant Homebrew trust only to the installed formula when required, then retry the upgrade instead of hiding the failure behind the agent updater.
- Desktop: image attachments no longer become conversation titles when a session has no name.

## [0.9.22] - 2026-11-16

### Fixed

- Desktop: the project board no longer loops while it measures its lanes.
  Before, the whole window could go blank after the laptop woke up
  offline or after a session view closed.

## [0.9.21] - 2026-11-16

### Added

- Desktop: Codex fast mode and all available thinking levels.

### Changed

- Desktop: a new agent session opens at once when you send its first
  message from a project, Home, the assistant, Review with agent or
  Work on it. The app keeps the start on this device and starts the
  agent in the background. The session shows your message with what
  it is waiting for or doing under it.
- Desktop: if the machine is offline, the start waits and runs when
  the machine comes online, also after the app restarts. A waiting
  start is listed with your sessions and can be cancelled.
- Desktop: a start that fails shows the reason, with Retry and Cancel.
- Desktop: the Add a machine sheet shows a much shorter join ticket
  (`beepjoin3`) for nodes and other desktops. It is the same ticket the
  phone QR code holds. Joining another desktop's mesh accepts it too.
  Tickets from older desktops still work.
- Desktop: Fix this on a failed Actions run and Continue on a pull request
  now open the harness and model picker under the button, like Review with
  agent and Work on it. Picking a model starts the session right away and
  opens it.
- Desktop: The right-click menu's Fix this and Continue still fill the
  project's prompt bar, so you can edit the prompt first.
- Desktop: the Home assistant starts on the machine it last ran on, as a
  project's start form does, while that machine is online and offers the
  assistant's agent.
- Desktop: Fix with an agent on a file sync conflict and on a blocked Git
  sync now opens the harness and model picker under the button, like Fix
  this does. Picking a model starts the session right away. A sync conflict
  is fixed on the machine that has it, and a blocked Git sync starts with
  both checkouts and recent sessions in the prompt. It used to open a draft
  in the start form.
- Desktop: Review with agent on Home's pull request cards and in Home's pull
  request list also opens the picker and starts at once in the project's
  checkout. The review still waits for your approval before it posts
  anything. Reply on Home still opens the start form.
- Desktop: Add folder from Add a project, Choose folder, is now a search
  palette of the machine's folders, the same picker used to add a project
  to a machine. Type to filter or type a path. The path shows as steps you
  can click back through. Checkouts have a branch icon, and a checkout with
  an origin remote offers "Add org/repo". It works the same on this
  computer and on other machines. Machines to pick from show at the top.
- Desktop: in Import a project, Choose opens the same folder palette.
- Desktop: "Keep Syncthing file sync" shows only for a folder inside a
  Syncthing share. Sync options in Add a project show only when a machine
  runs Syncthing.
- Desktop: The three buttons at the top of Add a project no longer have a
  highlight along their top edge.
- Desktop: Settings "Save files to" and installing a plugin from a folder now use the same folder picker as the rest of the app, instead of the system dialog.
- Desktop: The new project folder fields have a Choose button that opens the same folder picker.
- Desktop: a project opened in its own window for the first time now
  opens at the size of the last project window. Before, it nearly
  filled the screen.
- Desktop: project session history now loads all available sessions.
- Desktop: Show the copy button only on the latest agent reply.

### Fixed

- Desktop: a conversation just started no longer drops the message you
  sent and shows "Starting the agent" for a few seconds while its
  machine sets the model and thinking level.
- Desktop: a Claude Code turn that finished normally no longer shows
  "The turn did not finish" when the same step asked for the advisor
  next to a local command. The advisor request never ran, so that turn
  now ends normally.
- Desktop: switching the project on the start screen no longer replays the
  headline's words or briefly wraps it onto two lines, so the composer under
  it stays put.
- Desktop: when a part of the window fails to draw, only that part shows
  "This part failed to draw" with a Try again button. The rest of the
  window keeps working. Before, the whole window went blank until you
  restarted the app.
- Desktop: adding a project to a machine in a new folder inside a Syncthing share works instead of failing with "the target folder does not exist".
- Desktop: project links consistently support opening a separate window.
- Desktop: show files and changes from the agent's linked Git worktree.
- Desktop: sent images retain their detail and reopen at full quality.
- Desktop: the sidebar no longer says it is connecting to paired phones, which control the mesh and are never dialed.

### Performance

- Desktop: a finished painting on the start screen and empty pages wakes the
  app only when it moves, ten times a second, instead of on every screen
  refresh, so it uses less battery. It looks the same.
- Desktop: The folder picker reads three levels of folders in one request, so opening a subfolder shows at once. Folders read before show at once and refresh in the background.

## [0.9.20] - 2026-11-16

### Added

- Desktop: an agent reply in a coding agent session has a Copy button
  that copies the reply's text. The button appears once the reply has
  finished.
- Desktop: double-click the code of a text file in the file pane to edit
  it, with the cursor placed where you clicked.

### Changed

- Desktop: cards in lists (Home, project pages, the board's cells,
  notifications, the conversation switcher and Settings) scroll one card
  at a time. A short scroll moves one card, and a fast scroll moves
  several and slows into the card it ends on.
- Desktop: going to the next find match in a chat, or to the latest
  message, glides there, and the match lights up.
- Desktop: Home's section jumps and the diff's next file snap there
  quickly.
- Desktop: a machine removed from the mesh can be added back with a new
  pairing ticket. It keeps its identity instead of being refused with
  "this key was removed".
- Desktop: a project board's lanes show as many cards as fit on the page and
  scroll to the rest one card at a time, as on Home, instead of splitting into
  two columns. A lane with more cards than it shows hides the wanderer
  painting, so the cards get wider.
- Desktop: a painting keeps moving gently after it has painted in. Water and
  fields sway, and a few spots flicker in the light while the painting is on
  screen. It rests when it scrolls out of view or the window is hidden. The
  dawn haze behind the sidebar and lists stays still, and with Reduce motion
  on, the paintings stay still.

### Fixed

- Desktop: on Home, scrolling up with the pointer over a board cell
  scrolls the page again.
- Desktop: the Copy button under a quote at the end of an agent reply
  appears once the reply finishes, not while the quote is still
  streaming in.
- Desktop: the output of an opened tool call can be selected again.
- Desktop: a notification for a coding agent question that asks you to
  choose an answer no longer offers Allow and Deny. Clicking it opens the
  chat so you can pick an answer. This applies to system notifications
  and the bell.
- Desktop: answering a question in the chat marks its notification as
  answered right away, instead of leaving Allow and Deny showing for
  minutes.
- Desktop: a session that compacts its conversation shows "Compacting the
  conversation" instead of "Waiting for 1 command" when a background command
  is still running.
- Desktop: A message card in an agent session now reads "From <project>" for the sender's project, so it no longer looks like the session lives in that project.
- Desktop: in the diff viewer, pressing x folded the file at the top of
  the scroll instead of the file you clicked. Clicking anywhere in a
  file now makes it the current file, so x folds or opens that one.
- Desktop: On macOS, the text cursor in the chat composer and the new-session composer is now the same height as the text instead of taller than it.
- Desktop: selected code in the file editor is readable on light skins.
- Desktop: code in the file view can be selected.
- Desktop: syntax colours for keywords, strings and numbers are readable
  on light skins.

## [0.9.19] - 2026-11-16

### Added

- Desktop: The Assistant page's chats list can be resized by dragging its
  right edge or with the arrow keys on it. A double-click resets it, and the
  width is kept.
- Desktop: Shift+Tab in the assistant's composer, and in the new agent form,
  moves to the next machine.
- Desktop: when a newer beep-node release is out and online machines run
  an older one, the desktop shows a notification naming those machines.
  It appears once per release each time the app starts.
- Desktop (macOS): the notification has an Update button that updates
  every machine behind the new release. Each machine restarts once its
  agent sessions are idle. Clicking the notification opens Machines in
  Settings.
- Desktop: the main window and project windows reopen where you left
  them, at the same position and size, and come back maximized if they
  were maximized. A window last seen on a screen that is no longer
  attached opens in the default place instead.
- Desktop: a middle mouse click on a closable tab in the right panel
  (files, terminals, previews) closes it.

### Changed

- Desktop: on a project's pull request page, Review with agent opens the
  harness and model picker under the button, as the diff's review button
  does. Pick a model and the reviewer session starts and opens right away.
  While the review runs, the button reads Reviewing with a spinner.
- Desktop: on an issue's page, Work on it opens the same picker. Pick a
  model and a session starts on a new branch for the issue and opens.
  Right-clicking a pull request or issue row still fills the project's
  prompt bar, so you can edit the prompt first.
- Desktop: a new agent session in an empty project, and the Start a
  coding agent page, now show the project name and the machine picker
  right under the headline, above the task field. The machine picker
  moved up from the row under the field. It still shows an online dot
  and opens the machine menu. Before, the project and machine were only
  small text at the top left.
- Desktop: starting a conversation from a project's empty agent view
  shows your message as the first message at once, with a line saying
  the agent is starting on its machine. If the start fails, the input
  comes back with what you typed and the reason.
- Desktop: a conversation started from the project bar, Home, the
  assistant, Review with agent or Work on it opens with your message
  already in it, not an empty log until the machine answers.
- Desktop: on Home, each project's board lane shows two whole cards
  with one-line titles. Scrolling moves one card at a time, and at the
  first or last card the next few scroll steps are held so the page does
  not scroll by mistake.
- Desktop: messages that Claude Code sessions send each other with Claude
  Code's own messaging, and messages from Claude Code's agents, show in
  the chat as the same small "message from session" card as Beep's own.

### Fixed

- Desktop: terminals now use the colours of the picked skin: background,
  text, cursor, selection and the main ANSI colours. With a light skin the
  terminal is light too instead of staying dark, and open terminals change
  at once when the skin changes.
- Desktop: A chat started from Home's composer or the Assistant page opens on
  the Assistant page beside its chats instead of the plain session view.
- Desktop: a waiting message from another session shows in the composer
  queue with the sender's name and its words, instead of a "Steer" row
  with the raw tag.
- Desktop: a project's machine card showed two Fix with an agent buttons for
  the same Git sync failure, one by the sync status and one by the Git
  controls. The card now shows one.

## [0.9.18] - 2026-11-16

### Added

- Desktop: the PRs and issues tab on Home has a sort menu. Default keeps
  them by repository; Recent activity and Newest list open pull requests
  and issues from all repositories in one run, each row naming its
  repository. Your choice is remembered.

### Fixed

- Desktop: on light skins the instructions editor, reply box, version history preview and terminal screen show a light background instead of a gray one.
- Desktop: 0.9.17 did not ship for Linux because its build failed. The
  two built-in skins were missing from the Linux build. This version ships
  the Linux packages again, with the skins.

## [0.9.17] - 2026-11-16

### Added

- Desktop: plugins can be skins. A plugin with the new "Restyle the app"
  permission sets colours, fonts, corners, glass and paintings, with no code
  needed, and shows in Settings, Appearance to pick like any other skin.
- Desktop: two skins ship as plugins, Dot matrix and Hardware dark. They
  start off. Turn one on in Settings, Plugins, then pick it in Appearance.
- Desktop: text files in the side pane's file tabs can now be edited.
  Edit opens an editor with syntax highlighting. Save or Cmd/Ctrl+S
  writes the file back to the machine. If the file changed since it was
  opened, saving stops and offers Reload or Overwrite. Closing the tab or
  pressing Done with unsaved changes asks first.
- Desktop: the back and forward buttons on a mouse go one page back or
  forward, the same as Cmd+[ and Cmd+].

### Changed

- Desktop: buttons, chips, inputs, menus and cards take their corner rounding from the skin you pick, so rounder skins such as Classic round them too. The default Linear skin looks the same as before.
- Desktop: the assistant's message field on Home, shown on first start,
  before any projects, works like the other agent message fields. A
  paste of more than 10 lines becomes one "Pasted text" pill that you
  can preview, expand or remove. Text after a blank line is outlined as
  code.
- Desktop: the plan usage card shows when a limit resets as a
  countdown, such as "resets in 4h 12m" or "resets in 2d 8h",
  instead of a clock time or weekday.
- Desktop: the button that copies a file to the save folder is now
  called Download.
- Desktop: the Changes tab of a coding agent session no longer opens
  its changed-files list beside the chat on a wide screen. The new
  Files button in the diff toolbar fills the session with the pane,
  as Shift+Esc does, and shows the list.
- Desktop: with the Changes pane at full size, the Files button hides
  and shows the changed-files list, and the choice is remembered. In
  history and file sync diffs the list stays beside the diff on wide
  screens, and the same button hides it.
- Desktop: the project lanes on Home scroll without showing a scrollbar.

### Fixed

- Desktop: the plan line above the message field shows its status as a dot again instead of a long blue streak.
- Desktop: Mermaid diagrams in an agent's reply no longer flicker while
  the reply streams. A diagram draws once its block is complete and
  stays put as more text arrives.
- Desktop: a diagram no longer shows an empty strip along its right
  side.

## [0.9.16] - 2026-11-16

### Added

- Desktop: an Assistant page under Home in the sidebar lists the assistant's
  chats, running ones first and ended ones under a fold, beside the open
  chat. The assistant is an agent chat with no project that runs in a
  machine's home folder. New chat, or Cmd+N on Home, opens its composer.
- Desktop: Settings has an Assistant page to pick the assistant's harness,
  model and thinking level. The choice applies to every desktop and phone.
- Desktop: with no projects yet, Home shows the assistant with an "Import my
  projects" suggestion and the projects found on your machines, each with
  Import.
- Desktop: the assistant starts on this computer when it offers the harness,
  unless you pick a machine. Other machines need a newer beep-node.
- Desktop: the start screen and empty pages, such as Home with nothing
  running, show one of four paintings that changes each day: the sunrise,
  haystacks at dusk, the lily pond with its footbridge, or Parliament at
  sunset. The first run keeps the sunrise.
- Desktop: a Developer mode switch in Settings > General. With it on, the
  desktop updates to every release, patch releases included.

### Changed

- Desktop: a project page's board always shows Needs you, Working, Ready
  to review and Done today, so the lanes keep their place when a session
  finishes or asks something. The painting beside the lanes shows only in
  a wide window and no longer comes and goes with the cards.
- Desktop: Home sorts its project rows once after the app starts and keeps
  that order, also when you leave Home and come back, so a session
  finishing, asking or working no longer moves the rows; picking another
  sort sorts them again.
- Desktop: when a session window is too narrow for the chat beside the right
  pane, the pane folds to a strip at the right edge instead of squeezing the
  chat. The sidebar stays open.
- Desktop: the folded strip lists every tab (Files, Changes, Subagents, Device
  and others), not only Files. Picking one opens the pane over the chat; the
  fold button goes back to the chat.
- Desktop: widening the window again puts the pane back beside the chat as it
  was.
- Desktop: dimmed text, such as the board lane headers "Needs you" and
  "Working" and secondary details, is brighter and meets the WCAG 4.5:1
  contrast minimum.
- Desktop: in Add a project, clicking anywhere on a found project's row
  selects or deselects it, not only its checkbox.
- Desktop: without Developer mode, the desktop updates only to minor releases
  (such as 0.13.0) and skips patch releases.

### Fixed

- Desktop: the sidebar icons (Search, Home, Terminals, Settings) sit level
  with their labels instead of slightly low.
- Desktop: the notifications bell is centred in its button.
- Desktop: a board card's title is as far from the card's top edge as from
  its side.
- Desktop: screen readers and voice control announce sidebar items, project
  rows, machine rows, sort menus and the session title with the same names
  you see on screen.
- Desktop: typing `/` in the new agent field, on the start screen and in
  the project bar, lists the agent's slash commands even on a machine that
  has no running session of that coding agent. The list comes from a running
  session of the same agent on another machine.
- Desktop: a subagent card's result keeps its line breaks and sits under the
  card's title instead of running together as one block.
- Desktop: right-click menus, such as a file tab's menu in the side
  panel, stay open when the panel updates and restores its scroll
  position. They still close when you scroll with the mouse wheel or
  a touch drag.

## [0.9.15] - 2026-11-15

### Added

- Desktop: when a session moves to another machine and its project folder
  on the source machine has uncommitted edits, the move steps above the
  composer say so. Those edits stay on the source machine. Only the session
  and the worktrees it made move.

### Changed

- Desktop: while a session moves, the steps show "Waiting for checkouts
  to match" with both commits, then each worktree as it comes over
  ("Worktree name 1 of 2").
- Desktop: a project's board always shows the Working, Ready to
  review and Done today lanes, even when one is empty. Needs you
  appears only while something waits on you.

### Fixed

- Desktop: on the board (Home and a project's page), a Working card with
  a long running command now cuts the command with an ellipsis at the
  card's edge instead of spilling into the next lane.

## [0.9.14] - 2026-11-15

### Changed

- Desktop: adding a project to a machine where the chosen folder sits
  inside a Syncthing shared folder no longer fails. Beep adds the folder
  to Syncthing's ignore list on that machine and syncs it itself. The
  folder picker says so before you add. Projects set to use Syncthing are
  still refused.
- Desktop: on a project's board, sessions that ended today are a short list
  of one-line rows with an All button, instead of full-size cards.
- Desktop: the board's lanes sit together with no blank slot for an empty
  lane. In a wide window cards keep their size and a full lane takes a
  second column.
- Desktop: cards on a project's board leave out the project's name, and a
  working card says what the agent is doing.
- Desktop: a project board with little on it shows a painting of a wanderer
  above a sea of fog beside its lanes.
- Desktop: below 1100px wide, the window folds the sidebar to its icon rail
  and opens it again when the window is wider. The toggle still works.
- Desktop: when a session is too narrow to show its pane beside the
  conversation, a strip of Chat, Files, Changes and the other tabs shows one
  at a time, full width, instead of squeezing the pane under the chat.
- Desktop: a project with nothing on its board (no running agents,
  nothing done today, no sync conflicts) now shows the agent bar centered
  under the painted sunrise, with a welcome line like the new agent screen.
  It used to sit at the bottom of an empty page.

### Fixed

- Desktop: the sunrise behind the new task headline fades out softly
  inside the screen instead of being cut off at its left and right
  edges.
- Desktop: the dark oval behind the headline on the new task screen
  is now a soft shade that fades into the sunrise.
- Desktop: an agent that offers more than 500 models, such as flsh with several
  providers set up, shows all of them in the model picker instead of
  dropping the providers listed last.

## [0.9.13] - 2026-11-15

### Added

- Desktop: a step of a session's plan shows how many subagents ran under it
  and opens to their rows; a row opens the subagent.

### Changed

- Desktop: a session's plan is one line above the message field, with how
  many steps are done and the one that runs. It opens to the steps.

### Fixed

- Desktop: on Windows and Linux the window's title bar is dark with the
  dark skins and light with the light ones, instead of always following
  the system theme.

## [0.9.12] - 2026-11-15

### Added

- Desktop: Settings under Agent accounts, Claude Code, has a new Harness
  choice: "ACP adapter" (the default) or "Direct". The choice is saved for
  every machine, and new sessions use it. The Binary path and Launch
  arguments settings apply to the ACP adapter only; Direct runs the claude
  CLI found on the machine.
- Desktop: rename any machine from its menu in the sidebar, on Home or in
  a project's machines, or with Rename on its settings page. Every desktop,
  phone and node in the mesh shows the new name.
- Desktop: a Markdown file in the right panel has a Preview button that shows it rendered, like HTML files, and Source returns to the text.
- Desktop: in the Cmd+K search palette, the right arrow on a selected
  project lists its recent coding agent sessions. Enter opens the
  selected one, and the left arrow goes back to the search results.

### Changed

- Desktop: an agent reply's send time now sits at the top right of the
  reply instead of on its own line above it.
- Desktop: the board leaves out an empty lane instead of showing Nothing
  here.
- Desktop: with Project sync on, projects inside a Syncthing share leave
  Syncthing on every machine, including projects imported before and
  after, so only Beep sync writes them. The "Always use Beep sync over
  Syncthing" setting is gone. A project's menu can still hand it back to
  Syncthing.
- Desktop: adding a project to a machine, and Change folder, now picks the
  folder in a search palette of that machine's folders. The palette shows
  the path as steps you can go back through. A field filters the folders
  shown, or takes a typed path. Git checkouts have a branch icon. One row
  says exactly where the project will go. Up and Down move, Enter takes the
  highlighted row, Cmd+Enter (Ctrl+Enter on Windows) adds the project, and
  Backspace goes up one folder.
- Desktop: a problem report holds each machine's whole log for the chosen
  period, instead of the newest 1 MiB of each. A machine on an older node
  still gives its newest 1 MiB.
- Desktop: a problem report can be up to 95 MiB, up from 5 MiB, and a large
  one gets more time to upload.

### Fixed

- Desktop: a project window on macOS no longer puts the traffic-light
  buttons over the project name.
- Desktop: the project menu no longer offers "Use Beep sync instead of
  Syncthing" for a project whose folder is not in a Syncthing share on
  any machine.
- Desktop: in the model picker, a row is highlighted only when the cursor
  moves over it. A cursor resting on a row no longer highlights that row,
  and Enter picks the highlighted row.
- Desktop: an attached image opens in the viewer as the picture you
  attached, not the small preview that goes inline.
- Desktop: adding a project that uses Beep sync to a machine where the
  folder is inside a Syncthing share adds the Syncthing ignore rule there
  instead of refusing the folder.
- Desktop: with Project sync on, the import and add folder sheets show
  "Keep Syncthing file sync" off for a folder in a Syncthing share,
  matching what happens when it is left alone.
- Desktop: a project can be added to a folder that already holds its files
  without `.git`, for example files copied there by another file sync. The
  Git history arrives and the files stay as they are. Adding no longer
  stops with "has other files" or makes a nested `project/project` folder.
- Desktop: in the Import and Add a folder sheets, a long path in the folder
  browser's note no longer runs under its buttons.
- Desktop: on Linux, the AppImage no longer crashes at start. It now
  includes the tray icon library (ayatana-appindicator) instead of
  relying on the system copy. The .deb package depends on that library.

## [0.9.11] - 2026-11-15

### Added

- Desktop: the sidebar shows a "Star on GitHub" row above the footer.
  Clicking it opens the repository in your browser. The cross on the row
  hides it for good. Nothing is starred automatically.
- Desktop: coding agents can rig a textured humanoid 3D model so it can be animated, optionally remeshing it and setting its height. The `model_3d` tool's new `rig` action needs a saved Meshy API key, takes a GLB from the session folder or an HTTP(S) URL, and saves the rigged GLB in the session's working folder.
- Desktop: an iPhone or iPad plugged into a Mac shows in the Device tab
  next to the simulators, once it is paired and Developer Mode is on. Its
  screen updates about every two seconds. Taps, typing and buttons are not
  available yet.
- Desktop: agents see plugged-in iPhones and iPads in the device tool and
  can take screenshots, install and launch apps, and open a URL in Safari.

### Changed

- Desktop: the quick window takes the main window's look: monospaced details,
  a status as a dot and a word, and the same messages and message field.
- Desktop: the Beep tools for coding agents now have one tool per action, such as `list_machines`, `remote_run`, `remote_run_status`, `send_files`, `start_agent`, `device_tap`, `beep_logs`, `generate_image`, `generate_3d`, `rig_3d` and `list_models`. Each tool takes only its own arguments, so agents mix up arguments less often.
- Desktop: the `plugins` tool is removed, so coding agents can no longer list, create or call Beep plugins.

### Fixed

- Desktop: a session's usage in the quick window's header shows its context
  meter, tokens and cost apart instead of as one run of text.
- Desktop: in an agent session, Cmd+Shift+M opens the model picker with
  its search focused, so typing a model name picks it instead of going
  to the message field.

## [0.9.10] - 2026-11-15

### Changed

- Desktop: the log says where a failed incoming connection came from:
  its address, or its endpoint id when it came through a relay. Warnings
  for an incoming connection name its protocol and machine.

## [0.9.9] - 2026-11-15

### Added

- Desktop: a Windows machine lists its WSL distributions with the coding
  agents installed in each. Opening its page or the Add a machine sheet
  also looks into stopped distributions. Add a distribution as a machine
  with one button. Beep installs beep-node inside it and keeps it running,
  so agents installed in WSL can run there.
- Desktop: a WSL machine can be turned off and on again from its Windows
  machine's page or the Add a machine sheet. A turned-off one shows "Off"
  with "Turn on".
- Desktop: when a project's only checkout is on a machine where no coding
  agent can run yet, such as when the agent's adapter failed to install,
  the project bar names that machine and says why. A button opens it.

### Fixed

- Desktop: when no machine has a coding agent, Enter in the project bar and on the new agent page now says why nothing started and offers "Install a coding agent", which opens Coding agents to install Claude Code, Codex and others on a machine.
- Desktop: on Windows and Linux, errors that said "this Mac" now say "this computer". The Claude Code sign-in error also says to install it from Coding agents.
- Desktop: the sidebar section labels (Pinned, Projects, Machines) are as bright as Home and Terminals, so they stay readable over the sidebar painting.

## [0.9.8] - 2026-11-14

### Added

- Desktop: a machine's page shows its graphics cards with their video memory.
- Desktop: while an agent sends files to another machine with the `files`
  tool, its running row in the agent log shows how much has arrived, such as
  "412 MB of 1.2 GB". The count updates about once a second instead of only
  saying "Running".

### Changed

- Desktop: the phone pairing QR code holds far less data and is drawn larger,
  so a phone camera reads it at once. The copied ticket is unchanged.
- Desktop: the start form with a Project menu is gone. Chats now start
  inside a project. Cmd+N outside a project, Start an agent on Home, and
  New in the tray open Projects instead.
- Desktop: Review and Reply on pull requests and issues only show for
  repositories that are projects. Other repositories keep Import project.
- Desktop: the tray no longer opens a start form when nothing is waiting.
  It shows its empty list.
- Desktop: the model picker search matches the thinking level by its start, as
  it matches models: `opus 5.5 h` or `deepseek v4 med` shows rows such as
  `Opus 5.5 · High`. A version typed as `5.5` also finds ids written `5-5`.

### Fixed

- Desktop: a new chat in a narrow chat pane, next to the files panel,
  no longer scrolls sideways. The start screen fits the pane.
- Desktop: while a turn is stopping, the key hints under the composer keep
  their row, so Show more stays visible.

## [0.9.7] - 2026-11-14

### Added

- Desktop: coding agents on this Mac read a device's logs, list what its screen shows, install apps, open apps or links and clear an Android app's data with the `device` tool.

### Fixed

- Desktop: code blocks in chat replies no longer show a horizontal scrollbar over a one-line block.

## [0.9.6] - 2026-11-14

### Changed

- Desktop: every other block between triple line breaks in a message now goes out as code, not only the first.

### Fixed

- Desktop: a phone connected over adb to the Mac running the app shows a notification and appears in the Device tab at once, as it already did on other machines.

## [0.9.5] - 2026-11-14

### Added

- Desktop: a notification says when a phone connects to a machine over adb,
  naming the phone and the machine. Phones already connected when the app
  opens do not notify.

### Fixed

- Desktop: Shift+Cmd+M opens the model picker in an agent conversation, not only on the start form.

## [0.9.4] - 2026-11-14

### Fixed

- Desktop: a finished agent turn with an unknown length no longer shows a bare "Worked" heading.
- Desktop: the model picker's search field uses the same frosted glass as the rest of the picker instead of a dark band.

## [0.9.3] - 2026-11-14

### Added

- Desktop: Android phones show in the Device tab without Boot or Shut down
  buttons.
- Desktop: the Device tab's list updates as soon as a node reports a phone
  or emulator connecting or leaving. Otherwise it rereads every 60 seconds
  instead of every 10.
- Desktop: the harness, model and thinking picker (Shift+Cmd+M or the
  model pill) reads a search such as `opus 5 high`. Model words find the
  model across every harness on the machine, Claude Code and Codex first.
  A thinking level word such as low, medium, high, xhigh or max sets the
  thinking level, and a harness name such as `pi` keeps the search on that
  harness. Enter picks the first match at that level.

### Changed

- Desktop: built-in plugins start turned off on first run. Turn them on in Plugins.
- Desktop: Home's "PRs and issues" tab is now two plain lists, pull
  requests on the left and issues on the right, grouped by repository
  with one line per item. Review and Reply show on the row under the
  pointer. Each repository has a button that opens its project or
  imports it.
- Desktop: The label cloud, time filter, sort and issue picker are no
  longer on Home. They remain on the project page.

## [0.9.2] - 2026-11-14

### Fixed

- Desktop: in the search palette (Cmd+K), coding agent results found by their prompt show the matching prompt on its own line again, instead of squeezing the title, details and prompt onto one line.

## [0.9.1] - 2026-11-14

### Fixed

- Desktop: the Linux installers (Debian and AppImage) build again; the 0.9.0 Linux build failed because the example plugins were missing from the build.

## [0.9.0] - 2026-11-14

### Added

- Desktop: plugins in the plugins folder load at runtime and can place UI on the message box, such as the example cat. Plugins can also follow the open session and react when its turn ends: the example cat looks happy. Prototype: set `BEEP_PLUGINS_DIR` to use another folder.
- Desktop: plugins can add short text, buttons, switches and text fields to each session's row in the Coding agents list and to the open session's header, each knowing which session it belongs to. Plugin controls look like the app's own and work from the keyboard.
- Desktop: `beep.log` and `console.log`, `console.warn` and `console.error`
  lines from a plugin go to the Beep log file, tagged with the plugin's id.
- Desktop: a plugin can keep small data between restarts with `beep.store`.
  Removing the plugin deletes its data.
- Desktop: a plugin can be split over several files with relative imports.
- Desktop: a plugin names the plugin API version it needs in its manifest,
  `"api": 1`. A plugin that needs a newer Beep shows why in Settings,
  Plugins instead of running.
- Desktop: coding agents can list your plugins, read a plugin's state and
  recent log lines, and create new plugins. A new plugin waits until you
  allow it in Settings, Plugins.
- Desktop: a plugin with the "Give tools to your agents" permission can
  offer its own tools, and agents can call them.
- Desktop: a Plugins page in Settings lists your plugins with their source, permissions and state, and turns each on or off. Install one from a Git repository (`owner/repo`, an optional branch or tag and folder) or from a folder on your computer, update a Git plugin, or remove it after a confirmation.
- Desktop: a plugin shows what it asks for, in plain words, before it installs or updates, and an update shows the permissions it adds. Nothing runs until you allow it.
- Desktop: Beep ships a built-in plugin, the composer cat, a small animated
  pixel cat that sits on the message box. It is on by default and needs no
  approval. It updates with the app and cannot be removed.
- Desktop: the composer cat can be turned off in Settings, Plugins. The
  choice is kept.
- Desktop: the cat on the message box follows the session. It types while
  the agent works, shows a question mark when the agent asks something, and
  reacts to how each turn ended: happy when it finishes, startled when you
  stop it, sad when it fails, and yawns when it hits a limit.
- Desktop: plugins can follow a session's turn state and outcome, play
  one-shot sprite reactions, and fade or rise in and out.

### Changed

- Desktop: plugins cost nothing while idle. Animations are played by the page from
  a declaration, so the example cat no longer wakes its plugin for every blink.
- Desktop: plugin images are sent to the page once instead of in every update.
- Desktop: a plugin can start only when the slot it draws in appears, and one that
  draws only in slots stops a minute after they leave the page.
- Desktop: plugins pause while no window is visible, and animations stop and show
  their resting frame while the window is hidden or reduced motion is on.
- Desktop: plugins in the app data folder run only after you allow the permissions they ask for, and stop again when an update asks for more. Plugins in the folder named by `BEEP_PLUGINS_DIR` still run without approval, for development.
- Desktop: the Running tab shows one row per command the agent started, with the processes it spawned counted under it and listed when the row is clicked. Stop on a row stops the whole command. Rows show the command the agent ran instead of its shell wrapper, with shorter paths.

### Fixed

- Desktop: a plugin that uses more than half a second of script time in a minute is
  stopped with a reason instead of slowing the app.
- Desktop: a plugin's controls in the session header no longer cover the
  title, usage or state when the window is narrow. They hide when there is
  no room.
- Desktop: the new-agent start screen could scroll sideways, shifting
  the headline and composer under the sidebar. It now scrolls only
  vertically.

## [0.8.39] - 2026-11-14

### Changed

- Desktop: files attached to a new agent's prompt upload as soon as you
  attach them, not when you press Start. The new agent form and the
  project composer show the upload progress bar above the input and a
  progress chip per file, as in a conversation.
- Desktop: removing an attached file, stashing the draft, or leaving the
  new agent form without starting deletes the uploaded copy from the
  machine.
- Desktop: the model picker opens with ⇧⌘M (Ctrl+Shift+M on Windows and Linux) instead of ⌘M, which minimizes the window on macOS.

## [0.8.38] - 2026-11-14

### Added

- Desktop: the list of changed files beside a diff now shows how many
  draft review comments each file has, for example after a review with
  an agent, so they can be read before the review is sent.

### Changed

- Desktop: the worktree menu in an agent session's Files and Changes
  tabs lists the session's folder, the worktrees that session created
  and still exist, and the one currently shown. A "Show all" checkbox
  next to the search lists every worktree of the repository.

## [0.8.37] - 2026-11-14

### Changed

- Desktop: the move menu moves a session to the project folder on the
  other machine. It no longer offers a worktree there, and it no longer
  stops because a machine cannot make worktrees or ignores them in
  Syncthing. Committed branches still reach that machine through Git.
  Uncommitted work in a worktree stays where it is.
- Desktop: agents started by another agent now show on their project's
  page. Background agents (started with `background`) stay hidden there
  and appear only in the chat that started them.
- Desktop: notes such as "Switched to thinking medium from the next turn"
  show as dim status lines instead of looking like the agent's reply.
- Desktop: in a session, Cmd+1 still focuses the chat, and Cmd+2 to
  Cmd+9 now open the side pane tabs in the order they appear (Files,
  Changes, Running, Artifacts, Subagents, Device, then any opened
  files or terminals), where before only Cmd+2 to Cmd+4 worked.
- Desktop: Home's Machines tab shows one card per machine, with disk and
  memory bars, traffic and what the machine can run. Only terminals and
  projects that need a look are listed. The rest fold into one "projects
  here" line that opens a compact list.
- Desktop: on the Projects page, each found folder takes one line, with
  the machines that hold it shown as small chips. The path appears on
  hover.
- Desktop: pull requests and issues in repos without a project are
  grouped per repo and folded. Click a repo to see its items. Both
  sections fold from their header, and row buttons show on hover.

### Fixed

- Desktop: a file uploading to a session shows a progress bar along the top of the message field, also while a sent message waits for it.

### Performance

- Desktop: a project's branch list and each branch's upstream, ahead and
  behind counts are read directly from Git instead of starting a Git process
  for every branch whenever refs change. Branches refresh faster and use less
  battery.
- Desktop: the project page shows each machine's branch right away and
  updates the ahead and behind counts once the fetch finishes,
  instead of waiting for every machine to fetch first.
- macOS: the Beep download and update are smaller.

## [0.8.36] - 2026-11-14

### Changed

- Desktop: Home and Projects show a faint painted dawn band along the
  top. It stays above the cards and never covers them.
- Desktop: Home's board labels each project row with its name in small
  mono text, in place of a bold heading with a rule. Lanes with no cards
  have dimmer titles.
- Desktop: Projects group and organization headers use small mono text
  with tighter spacing. The "Add a project" row at the end of the list is
  gone once projects exist. The button in the title bar stays.

### Fixed

- Desktop: a session moved to another machine keeps its model instead
  of starting on the agent's default.

## [0.8.35] - 2026-11-14

### Performance

- Desktop: Git sync and the project and session views read branches,
  commits and repository folders directly instead of starting a Git
  process for each question, which uses less battery.

## [0.8.34] - 2026-11-14

### Added

- Desktop: each session remembers the emulator or simulator it last used, so its Device tab opens on that device again after a restart or on another desktop. A device that is no longer running is not shown and is forgotten.
- Desktop: Cmd+F in the Changes tab opens its filter, which now matches changed lines as well as file names. Matching lines are listed under each file; click one to jump to it.
- Desktop: a General setting, "Always use Beep sync over Syncthing", excludes every project from Syncthing on all machines, and each project imported later too.

### Changed

- Desktop: a session's project, branch and machine show on a line under
  its title, so a long title and a long project name are no longer cut
  against each other.
- Desktop: an agent's sign-in shows numbered steps (open the page, paste
  the code) with the link on one line, instead of the whole address in a
  tall box.

### Fixed

- Desktop: file and attachment uploads keep running while bytes transfer, and time out only after 30 seconds without progress.
- Desktop: the code in a diff can be selected and copied again.

### Performance

- Desktop: a synced Git project checks only the branches that moved
  when another machine announces its branches, instead of running Git
  for every branch.
- Desktop: a file or Git view whose read keeps failing retries less
  often instead of up to four times a second.
- Desktop: checking whether a path lies in a session's worktrees runs
  Git again only after worktrees are added or removed.
- Desktop: idle agent sessions share one process list read about once
  a minute instead of each reading its own.

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
