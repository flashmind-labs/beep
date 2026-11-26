# Node changelog

Earlier releases predate this changelog.

## [0.8.29] - 2026-11-26

### Fixed

- Antigravity sessions now compact at the auto-compact percent or token count set in Beep.

## [0.8.28] - 2026-11-25

### Added

- Node: a folder's update now says which paths changed, so desktops read only what a change concerns.

## [0.8.27] - 2026-11-25

### Added

- Node: A session can carry on an earlier one on another harness. It reads the earlier transcript before its first answer, and the earlier session closes.

### Fixed

- Node: Android phones appear in the device list when adb is installed after the node started, without restarting it.
- Node: Issue pickup no longer starts a second agent on an issue you assigned to yourself, or on one a session already works on when its first prompt names the issue by its link or as "issue #N".

## [0.8.26] - 2026-11-25

### Added

- Node: on Linux and macOS, sessions give their agent a Docker socket of
  their own, so the containers it starts with `docker run` or
  `docker compose up` show in the session with their published ports.
  Closing the session stops them, and nothing is removed. It applies to
  sessions started after the setting changes.
- Node: Beep ships a `report` skill in its skills folder, next to GitHub and Forgejo, and tells agent sessions to use it to generate their final response. Delete the installed skill file to turn this off; edit it to change what agents follow.

### Changed

- The desktop on this machine hears of a team invite the moment it arrives or is dropped, instead of asking again on a timer.
- A terminal's state reaches a window that follows that one terminal alone, instead of the whole terminal list.
- A machine that acts for no team no longer wakes every 10 seconds to elect a team's gateway: it elects when the mesh, a roster or a host's reach changes.
- Agent spend figures update when an agent's log changes, instead of on a 60 second timer.
- Asking a node to sync a project returns as soon as the project settles, with no half-second wait loop.
- Syncthing coming back, or changing a folder, is noticed from its event stream instead of a 5 minute recheck.

### Fixed

- Linux, macOS and Windows: a team invite or card that is a day old goes at once, not at the next read.
- Linux, macOS and Windows: a team keeps a machine acting for it after the acting machine steps down.
- Linux, macOS and Windows: the spend report ends with an error, not silently, when the agents' logs cannot be watched.
- Linux, macOS and Windows: one background task's output and exit reach a desktop that follows only that task.
- Linux, macOS and Windows nodes: a parent agent waiting on an agent on another machine now hears when it finishes even if the first message was lost on a dropped link.
- Linux, macOS and Windows nodes: a deleted conversation no longer comes back in the list for up to a minute.
- Linux, macOS and Windows nodes: background agents that finish are removed at once, not up to a minute later.
- Linux, macOS and Windows: a slow or stuck disk no longer delays a machine's other status reports.
- Linux, macOS and Windows: one unreachable peer no longer holds up the other file fetches of a synced project.
- Linux, macOS and Windows: a team change or a project import reaches the team board without waiting for a slow desktop.

### Performance

- Node: the session list sent to a desktop or phone no longer carries token counts, context use, touched files or the rest of the last message, and a desktop that understands it gets only the sessions that changed. A node with several running sessions sent about 75 KB every second before, which slowed Stop, queued messages and session views on remote machines.
- Linux, macOS and Windows nodes: several open usage views share one read of the agents' logs instead of one each.
- Linux, macOS and Windows: the device list is read once per change, and only polled while a simulator is listed or a device boots.
- Linux, macOS and Windows: terminal output no longer makes every listener read the terminal list again.

## [0.8.25] - 2026-11-24

### Added

- Node: Added the `team_join` MCP tool (with user confirmation) and the built-in `beep_join_team` skill so coding agents can help users join teams and troubleshoot team setup.

### Fixed

- A wait on an agent running on another machine now ends the moment that agent settles, with no 5 second lag, when both machines run this version.
- A call or ping sent right after switching watched topics is answered without waiting for the topic lookup.
- A project's file view no longer stays on an old folder when two devices watch it at once.
- A team's board and your teammates' cards keep updating while a machine of the mesh or the forge is slow to answer.
- A team gateway acts on a changed team or project as soon as the mesh reports it, not at its next ten-second round.

### Performance

- The list of tools with a newer release is ready sooner after the node starts, because the releases are looked up together.

## [0.8.24] - 2026-11-24

### Fixed

- Node: Pi sessions now register Beep's MCP server directly through Beep's Pi extension so Pi can call `start_agent`, `list_machines`, `list_projects`, and other Beep tools even though `pi-acp` drops `mcpServers`.

## [0.8.23] - 2026-11-24

### Fixed

- Node: Pi sessions now recover from provider `content_filter` stops by informing the agent within the same turn, and surface unrecovered provider errors instead of ending with blank output.

## [0.8.22] - 2026-11-24

### Added

- Antigravity sessions started outside Beep now show in a project's sessions and in the folders suggested when adding a project, and can be continued in Beep.

### Fixed

- Opening a file in a folder known only from the agents' logs no longer waits behind a session listing.
- CI pass and fail notices no longer wait for a machine that does not answer.
- If the agent refused a queued message you had taken back to edit, Beep no longer sends the old version as your next prompt.
- Node: When Claude Code starts working on its own, such as after a
  background command finishes, the chat now shows it as working instead of
  idle.
- Antigravity multiple-choice questions no longer appear twice in Beep, and typing your own answer fills the free-form field.

### Performance

- Sessions started outside Beep are read from disk once per change instead of twice, so lists and search catch up sooner.
- A desktop or phone watching files on a machine with many projects gets its first data sooner, because the machine looks up the folders together.

## [0.8.21] - 2026-11-24

### Added

- Subagents started through pi's `subagent` tool now show as subagents, with their agent, task and model, in live and saved pi chats.

### Fixed

- Node: With two desktops or phones open, each now gets every usage and spend update.
- Node: A session moved to another folder resumes in that folder after a node restart.
- Pi's compaction summary now shows as the compaction note instead of being appended to the agent's last reply.
- Taking back a waiting message while the agent is still receiving it now removes it, and it is no longer sent again with its old text.

### Performance

- Node: A wait on a sub-agent or review returns as soon as it finishes.
- Node: Reopening many sessions after a restart now happens at the same time for sessions that do not depend on each other.
- Node: Resuming, deleting or moving one session no longer waits on another session.
- Node: Moving several sessions to one machine places their worktrees at the same time.
- Node: Files and Changes views open without waiting for file watching to finish starting.
- Node: Work on one synced project no longer waits behind work on another project.
- Node: Credentials and agent settings from the mesh apply without waiting for projects to load.
- Node: CI checks for one project no longer wait behind slow forge calls for other projects.
- Node: Installing a tool from the desktop no longer waits for pinned tools to install.

## [0.8.20] - 2026-11-24

### Added

- Node: accept thinking levels for Antigravity sessions and report per-model thinking levels from Antigravity's model catalog.
- Node: Lazily runs installed language servers (such as `rust-analyzer`,
  `vtsls`, `typescript-language-server`, `basedpyright`/`pyright`, `gopls`, and
  `clangd`) per workspace with an idle timeout to answer editor go-to-definition
  requests.

### Changed

- Node: agent sessions now receive structured Beep ADE framing, runtime context, explicit precedence for Beep links and artifacts, and clean MCP initialize instructions.

### Fixed

- Node: short metrics queries work even when older retained logs exceed the scan limit. Summaries show the observed times in UTC.

### Performance

- Node: when one agent starts several agents at once, they now start side
  by side instead of one after another. The limit of 8 agents under one
  session still applies.
- Node: session lists, project session pages, search and file previews no
  longer each read every coding agent's session files from disk. The node
  reads them once and shares the result, which cuts its disk work when many
  sessions are stored.
- Node: listing sessions no longer looks through every Claude Code project
  folder for each running session.

## [0.8.19] - 2026-11-24

### Added

- Node: metrics for how long each internal worker's messages wait and take
  to handle, and how long its background jobs take (process scans, git
  reads, sync changes). `beep_metrics` shows them as `beep_actor_wait_seconds`,
  `beep_actor_handle_seconds` and `beep_actor_job_seconds`.
- Node: a log line when one of those messages waits or runs long, naming the
  worker and the kind of message.

### Changed

- Node: the log of a failed request now says whether the node refused it,
  with the reason, or could not send the answer. A stream that ends is no
  longer logged as a slow or cancelled request.

## [0.8.18] - 2026-11-23

### Fixed

- Node: if the desktop or phone disconnected while a project's file sync was
  starting over, the project stayed off until the node restarted. The start
  over now finishes.
- Node: a project no longer disappears from the sync status for a moment
  while its sync is stopped and started again.
- Node: a terminal that exits stays in the list for 30 seconds, so every
  connected desktop and phone sees that it ended. Before, only the first
  one to look saw it, and it vanished for the others.
- Node: a background command that was still starting when the desktop app
  quit is now stopped with the others instead of running on.
- Node: a port that a session's command opens after one desktop or phone
  already looked is now shown to the others too.

### Performance

- Node: sessions in one folder and every connected desktop or phone now
  share a single git read of its changes, instead of each running git for
  the same diff. Opening Changes again answers without running git until
  the folder changes.
- Node: refreshing a conversation's Files tree no longer runs git once per
  open folder for each connected desktop or phone. Git's status is read once
  per change of the checkout and shared.
- Node: a change in one session's running commands no longer makes the
  other sessions' Running lists read again.

## [0.8.17] - 2026-11-23

### Fixed

- Node: Pi model lists include provider metadata for model pickers.
- Node: saved instructions now work with Pi and agents without system-prompt support.

## [0.8.16] - 2026-11-23

### Added

- Node: beep-node serves a team's board to teammates over a new peer-to-peer
  link, with no server in between.
- Node: agents get `team` and `team_ask` tools. `team` shows the
  team's work. `team_ask` messages a teammate's agent, at most 10
  messages an hour per session.
- Node: beep-node starts an agent on each issue newly assigned to you on
  GitHub or Forgejo, once per issue. An optional limit caps how many
  of these agents run at once.
- Node: everyone on a team must be able to push to the repository, when
  they join and after. A teammate is taken on only when GitHub or Forgejo
  confirms they can push (on Forgejo, where only admins see others'
  access, their push proof counts). Once a day beep-node checks you can
  still push, and leaves the team for you when you can't.

### Fixed

- Node: report each model group's five-hour quota when Antigravity provides it.
- Node: Antigravity sessions start and resume with saved instructions.
- Node: replacing a move rolls back the earlier uncommitted attempt first.
- Node: stopping a terminal after its process exits preserves the actual exit result.

## [0.8.15] - 2026-11-23

### Added

- Node: apply the synced flsh OpenRouter stream idle timeout. The default is 300 seconds.

### Fixed

- Node: agents receive saved instructions in the system prompt. An adapter that cannot apply them reports an error before the chat starts.

## [0.8.14] - 2026-11-23

### Added

- Node: discover and install root and nested skills independently from the same repository.
- Node: reports whether Antigravity is signed in on its machine and runs
  its sign-in for a desktop.
- Node: the node reads and edits the MCP server list that Antigravity
  keeps for the desktop.
- Node: counts what Antigravity sessions spend, read from Antigravity's
  own conversation history on this machine. Antigravity shows in the
  Usage view's "By harness" and in per-session spend, as a subscription.
  Requests from Antigravity's subagents count toward the session
  that started them.

### Changed

- Node: dated usage and historical session totals are reported separately.
- Node: Gemini CLI is no longer detected, installed or run as a harness. Use
  Antigravity instead.

### Fixed

- Node: assistant usage includes billed OpenRouter costs across models and keeps each machine's share when a chat moves.
- Node: resuming an older flsh conversation no longer counts its lifetime cost as spending today.
- Node: watched folders report nested file edits, including changes that keep the same file size.

## [0.8.13] - 2026-11-22

### Added

- beep-node: an assistant chat can start a thread as a custom agent, such as
  a `.claude/agents/researcher.md` file or a Codex, OpenCode, pi or flsh agent
  file, from the chat's folder or the user's agents on that machine. The
  thread follows that agent's instructions, runs on its harness unless the
  chat names another, and keeps the agent across restarts and moves; an
  unknown name is refused with the names that exist.

### Fixed

- Node: recognize a library with a top-level `SKILL.md` as one installable skill.

## [0.8.12] - 2026-11-22

### Added

- Node: library skills picked for an assistant chat are installed in the
  chat's folder and in each of its threads' folders, including threads started
  later. They follow the chat when it moves to another machine.

### Changed

- Node: An assistant chat ends each thread once it has its answer, and reuses
  a thread that is already working on the same thing instead of starting
  another.

### Fixed

- Node: Claude conversations retain their scrollback after compaction and restart.

## [0.8.11] - 2026-11-22

### Changed

- Node: assistant chats now hand off work only as threads the user can
  see and open. The coding agent's own hidden subagent tools are
  switched off in those chats for Claude Code, Codex and OpenCode, and
  for flsh once it supports it.

## [0.8.10] - 2026-11-22

### Added

- Node: a machine that takes a chat says how much of its files have come,
  so the moving machine can show the copy's progress.

### Fixed

- Node: when a chat's files cannot be read yet, both machines log why at
  warning level instead of retrying silently.

## [0.8.9] - 2026-11-22

### Added

- Node: agents can name the agents they start. The assistant starts each
  thread of a chat with a summary of the conversation so far.
- Node: when you write to a session, its agent first hears what the agents
  it started came to since its last turn. An agent started by another hears
  what that one last said.
- Node: each new assistant conversation gets its own folder under
  `~/.beep/assistant/`, and each of its threads a subfolder named after it,
  for notes and findings. Deleting the conversation removes its folder.
- Node: a moved chat arrives with its threads and its folder's files.
- Node: running threads resume on the new machine, still linked to their
  chat.
- Node: ended threads arrive ended and are listed under their chat. They are
  not started again.
- Node: a chat that moves away does not stop the threads that move with it.
- Node: the assistant has its own tools for a chat's threads:
  `start_thread`, `thread_status`, `send_to_thread`, `stop_thread` and
  `list_threads`. A thread starts on the chat's machine, in a subfolder of
  the chat's folder, on the chat's harness unless another is named.

### Changed

- Node: an agent that another agent started no longer makes its idle parent
  run a turn when you wrote to it or stopped it. The parent hears of it at
  its next turn instead.

### Fixed

- Node: the assistant no longer uses its harness's hidden subagents in place
  of threads, so the threads it says it started show under the chat.
- Node: flsh is installed and updated in `~/.local/bin`, where flsh's own
  installer puts it, instead of beep-node's own folder. A copy left in
  beep-node's folder moves there, or is removed when `~/.local/bin` already
  has flsh. Before, sessions kept running the node's older copy after flsh
  was updated with its own installer, so assistant chats on flsh could not
  reach Beep's tools and started no threads.
- Node: a resumed session keeps its title. Before, flsh could rename it
  after the resume from Beep's context text, such as the user's standing
  rules.

## [0.8.8] - 2026-11-22

### Added

- Node: Google's Antigravity CLI (`agy`), a coding agent harness, is
  supported. The node finds it on the machine and starts it over ACP
  with every tool approved. When it is missing, the node installs it
  with Google's install script, and Antigravity updates itself.
- Node: a session's running commands report which of their ports
  answer as a web page, so the desktop can offer the right one.
- Node: the node reads how much of Antigravity's weekly limits is left
  for each model group: Gemini models, and Claude and GPT models. It
  reads this from the Antigravity CLI, so checking uses none of your
  quota.

### Fixed

- Node: after a Claude Code session resumes, for example after the
  node restarts, its Files and Changes tabs stay on the linked
  worktree the session works in instead of jumping back to the main
  checkout.
- Node: a Claude Code session resumes in the folder it was started in,
  even after it worked in a worktree that was later removed. If that
  folder is gone too, it resumes in the home folder. Resuming no longer
  fails with "Recreate that folder to resume it."
- Node: a server that a coding agent starts shows its port in the
  Running tab within about a second of it listening, instead of up to
  a minute later.
- Node: in a Claude Code session, when the agent keeps working on its
  own after a background task finishes, without a new prompt from you,
  each of its messages now shows as a separate message with its own
  time. Before, they ran together into the previous message with no
  space between them.

## [0.8.7] - 2026-11-21

### Added

- beep-node: a session cannot start or resume with a harness that its project
  does not allow. This applies when you start it, when another agent starts it
  with start_agent or review, and when a session moves in. The error names the
  allowed harnesses.

### Changed

- beep-node: the harness "Can start" limit no longer applies to agents that
  a session starts. Limit harnesses per project instead.

### Fixed

- Node: usage and cost views counted all of a flsh session's spend in
  the hour of its last message, so a resumed session showed its whole
  cost as today's. Each flsh turn's spend now counts in the hour it
  happened, for sessions written by a flsh version that logs turns.
  Older sessions keep their earlier spend in one lump.

## [0.8.6] - 2026-11-21

### Added

- Node: flsh's config now carries each model's own quantizations.
- Node: before pi starts, the OpenRouter routing settings are written into
  pi's models.json, and the user's other entries are kept. A models.json
  with comments is left alone, and pi does not start. The error says so.

### Fixed

- beep-node: A subagent started with start_agent and then stopped, whose
  session is no longer on its machine, now shows as ended in its parent's
  Subagents list instead of working forever.

## [0.8.5] - 2026-11-20

### Added

- Node: beep-node runs a session signed in as the named account it was
  started with, and keeps that account when the session resumes or the
  node restarts. Agents and reviewers that a session starts use the
  account of the same name.
- Node: beep-node refuses a start on an account it does not have yet.
- Node: beep-node reads the plan limits of each named account it holds,
  apart from the default account's.
- beep-node: refuses a start_agent or review child agent whose harness the
  parent's harness may not start. The error names the harnesses the parent
  may start.

### Changed

- beep-node: a node tries to reach another machine, and syncs files with
  it, again every 5 seconds instead of waiting up to a minute. Each try
  gives up after 10 seconds.

## [0.8.4] - 2026-11-20

### Added

- Node: Codex can run on a Direct driver. beep-node now drives
  `codex app-server` itself instead of the npm ACP adapter, so Codex
  sessions no longer need the adapter or Node.js. Modes, model, reasoning
  effort, fast mode, approvals, questions, steering, compaction,
  proposed plans and history, images included, work as before. Codex
  inside WSL keeps using its adapter.
- Node: on the Direct driver, a Codex command's output shows in its tool
  entry.
- Node: a machine with the Codex CLI but no Codex ACP adapter or npx
  runs Codex on the Direct driver instead of leaving it out, as Claude
  Code already does.
- beep-node: release archives include an Intel Mac build
  (x86_64-apple-darwin). The install script and the node's self-update
  pick it on Intel Macs instead of refusing.

### Changed

- Node: Claude Code and Codex on the Direct driver, and coding agents
  inside WSL, now run inside the node itself. beep-node no longer starts
  a second copy of itself for each session.

### Fixed

- Node: a pi session's header shows what it has cost so far, updated as
  it works.
- Node: a finished agent review no longer sends a notification that opens a session that is already gone.

## [0.8.3] - 2026-11-19

### Fixed

- Node: resumed Codex sessions use the selected model from the start, avoiding a misleading model mismatch warning.
- Node: skill libraries added from the desktop install on every machine,
  even one that cannot reach the library's Git repo. Only the machine that
  added the library clones it; the others copy the skills from a peer.
- Node: a pi session no longer opens with the adapter's startup message
  listing its version, context files and skills.
- Node: the agents list shows when a newer pi version is out. It checks the
  current pi package instead of the deprecated one.
- Node: a pi session no longer shows a separate thinking mode pill beside
  the model pill, which already shows the thinking level.

## [0.8.2] - 2026-11-19

### Added

- Node: Manage user MCP servers in Codex and Claude Code’s native configuration through Beep settings.

### Changed

- Node: Beep's bundled GitHub and Forgejo agent skills now live in Beep's
  own skills folder, `~/.config/beep/skills` (or
  `$XDG_CONFIG_HOME/beep/skills`; the config folder on Windows), instead of
  the folders Claude Code, Codex and Hermes use. Agents Beep starts learn
  about every skill in that folder, so skills you add there reach only
  Beep's agents.

### Fixed

- Node: Flashmind session usage continues reporting known costs when some calls have no price.
- Node: sends the actual question with Codex answer choices.
- beep-node: on networks that block DNS over TLS, lookups now use encrypted
  DNS over HTTPS instead of the network's own DNS. A name that public DNS
  says does not exist is no longer answered by the network's DNS, which some
  hotel and filtering networks rewrite.

## [0.8.1] - 2026-11-18

### Added

- Node: applies a project's sync rules from the mesh after its
  .beepignore, and rescans the project when those rules change.

### Changed

- Node: downloads are available from GitHub Releases, and container images are published to GitHub Container Registry.
- Node: deleting a session that is open in a terminal or IDE first ends the process holding it, asking it to exit and stopping it after 5 seconds. A session with no known process still refuses to delete.
- Node: read-only PR reviews inspect the PR head by ref and keep the current checkout intact.

### Fixed

- Node: ended assistant chats are remembered across restarts and
  reported as assistant chats, so they stay listed on the Assistant
  page after they end or the node restarts. Before, only chats still
  running showed there.
- Node: Conversation search includes supported agent histories outside imported projects.
- Node: file sync no longer misses changes after abrupt restarts.

## [0.8.0] - 2026-11-17

### Added

- Node: on a Windows machine, Codex, Gemini CLI, pi, flsh, Hermes and
  OpenCode installed in a WSL distribution now run as that machine's
  agents when Windows lacks them, as Claude Code already did. They need
  their ACP adapter or npx installed inside WSL.
- Node: sessions that coding agents ran inside WSL now show in Beep's
  session lists and transcripts, under their Windows folders.
- Node: keeps a search index of the prompts and agent replies in the
  sessions of its project folders. It reads the agents' own files on that
  machine.
- Node: stream supported device video using native H.264 capture, with JPEG fallback.
- Node: serves port forwards to the user's desktops. A desktop can reach
  only ports on the node's own loopback, and only while the machine is
  shared with other devices.
- Node: a server that a session's command starts on a port after the
  session launched now appears in the desktop's Running tab within about a
  minute, because the node checks its sessions' ports once a minute.
- Node: reports which large file a sync waits on and how much of it has
  arrived, so desktops can show it.
- Node: When a coding session's turn ends, the node counts the lines
  added and removed in its checkout, as the Changes tab does, and sends
  them with the session so the desktop can show them beside it.
- Node: Codex fast mode and all available thinking levels.
- Node: beep-node can replace a text file in a working folder when the
  desktop's editor saves it, keeping the file's permissions. Older nodes
  refuse this, so update the node to save edits.
- Node: assistant chats get instructions for being the Beep assistant and
  project tools: `projects_hosts` lists Git hosts, `projects_create` creates
  a project (private repository unless asked), `projects_clone` clones a URL,
  `projects_found` lists the projects found on your machines and
  `projects_import` imports them.
- Node: the project tools need a desktop to be online. Agents that the
  assistant starts do not get them.
- Node: an assistant chat stays one after beep-node restarts. A chat moved
  to another machine keeps its title, model and thinking level.
- Node: on a Windows machine without Claude Code installed on Windows,
  the node now runs the Claude Code installed in a WSL distribution as
  that machine's Claude Code. Sessions work on the project's Windows
  folder, seen from WSL under `/mnt/`. The machine's Claude Code sign-in
  is passed into WSL, and Beep tools work there. The WSL distribution no
  longer needs to be added as a separate machine.
- Node: with a Meshy API key saved, coding agents can animate, retexture,
  remesh, convert, resize, UV unwrap, split and prepare 3D models for
  printing, and make reliefs and Meshy images.
- Node: moving a session reports to the desktop when the project folder on
  the source machine has uncommitted edits. Those edits stay on the source
  machine. Only the session and the worktrees it made move.
- Node: a session's plan steps carry when they started and ended, so the
  apps can show each step's subagents.
- Node: Claude Code can run directly on the `claude` CLI instead of the npm
  ACP adapter, when its harness is set to Direct. Prompts, streamed text and
  thinking, tool calls, tool permission prompts, plan approval, Claude's
  questions (shown as forms when the app supports them), model and mode
  changes, stopping a turn, and resuming a session with its history all work.
  Claude Code also runs on a machine that has its CLI but neither its ACP
  adapter nor Node.js/npx, through the node's own Direct driver.
- Node: coding agents can rig a textured humanoid 3D model so it can be animated, optionally remeshing it and setting its height. The `model_3d` tool's new `rig` action needs a saved Meshy API key, takes a GLB from the session folder or an HTTP(S) URL, and saves the rigged GLB in the session's working folder.
- Node: a Mac node lists the iPhones and iPads plugged into it, so they
  appear in the Device tab and to agents. The screen, app installs and
  app launches work as on the desktop. Taps, typing and buttons are not
  available yet.
- Windows: the node runs the WSL machines it added only while Beep may use
  the computer. It leaves one alone when another Beep on the PC already
  runs it. If you stop WSL, for example with `wsl --shutdown`, the node
  waits until the machine is turned on again or the node restarts. A
  distribution removed from WSL is forgotten.
- WSL: a node running in WSL is named after its distribution and the
  Windows PC, such as Ubuntu on gpu-box.
- Node: when an agent sends files to another machine, the sending node reports
  how many bytes the receiving machine has fetched. The agent session shows
  this as progress on the running `files` tool call, updated about once a
  second.
- Node: agents read a device's newest log lines with the `device` tool's
  `logs` action: logcat for Android phones and emulators, the last five
  minutes of an iOS simulator's log, with an optional filter and line count.
- Node: the `device` tool's `ui` action lists what the screen shows, each
  element's label, id and where to tap it, for Android devices and iOS
  simulators.
- Node: the `device` tool installs an APK or `.app` already on the device's
  machine (`install`), starts an app or opens a link (`launch`), and stops an
  Android app and clears its data (`reset`).
- Node: desktops can follow the machine's phones and hear when one connects.
- Node: when a coding agent sends a Beep problem report, the approval
  offers "Send and open issue". It also opens a public GitHub issue
  with your signed-in `gh` account. The issue carries the description,
  the report reference and the versions, but no logs. The agent gets
  the issue link back.
- Node: coding agents read Beep's status and logs first, and offer a
  problem report when Beep itself fails.
- Node: an Android phone connected to the machine over adb, by USB or
  wireless debugging once debugging is allowed on the phone, is listed with
  the machine's devices. The Device tab and agents' `device` tool show its
  screen and tap, swipe, type and press keys on it. A phone cannot be booted
  or shut down from Beep.
- Node: the machine tells desktops when adb sees a phone or emulator connect
  or disconnect, so the device list updates without waiting for a refresh.

### Changed

- Node: updates initiated from Beep follow Developer mode.
- Node: desktop controllers without hosting remain mesh controllers and no longer appear as coding agent or file hosts, or as phones.
- Node: agents share Cargo build artifacts across linked worktrees.
- Node: a Windows node no longer installs or runs beep-node inside WSL.
  WSL machines added earlier stop running and can be removed from Machines.
- Node: Agents are instructed to use image content and the request when titling sessions, while preserving explicit titles.
- Node: when a coding agent session moves to another machine, the note the
  agent gets now names the machine it came from and the machine it runs on
  now. Before, it only named the old and new folders.
- Node: an agent on a node can now read every machine's status, logs and
  metrics and include them in a problem report, as an agent on a desktop
  can. Before, a node refused these requests from another node, so the
  beep_logs, beep_status, beep_metrics and report tools only covered that
  node. Machines still on an older node release keep refusing until they
  update.
- Node: when a coding agent session moves to another machine, the note the
  agent gets now names the machine it came from and the machine it runs on
  now. Before, it only named the old and new folders.
- Node: `beep-node mesh join` accepts the short `beepjoin3` ticket. The
  older `beepjoin2` ticket still works, but desktops no longer make it.
- Node: Beep now keeps every project it excludes from Syncthing in one `// Beep project files` block in the shared folder's ignore file, not one block per project. Old blocks merge on the next change.
- Node: session history metadata is available in pages.
- Node: `beep-node mesh join` with a fresh ticket brings a removed node
  back into the mesh.
- Node: moving a session no longer waits for every file of the project.
  The target waits until its checkout is at the same commit as the
  source's.
- Node: the worktrees a session created move with it, including their
  branch, commits and uncommitted changes. Other worktrees stay where
  they are. The source keeps its copy of a moved worktree, detached from
  the branch.
- Node: a Claude Code session shows its task list as the session's plan
  again. Claude Code had its task tools off on recent models.
- Node: with Project sync on, adds the Syncthing ignore rule for each
  project in one of its shares unless the project chose Syncthing.
- Node: a node gives a problem report its whole log for the chosen period,
  instead of the newest 1 MiB.
- Node: a problem report can be up to 95 MiB, up from 5 MiB, and a large
  one gets more time to upload.
- Node: the Beep tools for coding agents now have one tool per action, such as `list_machines`, `remote_run`, `remote_run_status`, `send_files`, `start_agent`, `device_tap`, `beep_logs`, `generate_image`, `generate_3d`, `rig_3d` and `list_models`. Each tool takes only its own arguments, so agents mix up arguments less often.
- Node: the `plugins` tool is removed, so coding agents can no longer list, create or call Beep plugins.
- beep-node: the log says where a failed incoming connection came from:
  its address, or its endpoint id when it came through a relay. Warnings
  for an incoming connection name its protocol and machine.

### Fixed

- Node: minor release notes now include changes from preceding patch releases, even without pending entries.
- Node: resumed Claude sessions recover available earlier messages from native history.
- Node: an interrupted Git transfer no longer leaves half-written pack files
  in `.git`. A stalled transfer gives up after 30 seconds of silence instead
  of waiting five minutes. Git packs its objects together once many have
  arrived.
- Node: a machine with a very long system journal answers status checks
  at once, so beep_status no longer shows it as not answering.
- Node: a resumed session shows its added and removed line counts
  right away after a restart, not after its next turn ends.
- Node: file paths in a WSL session's tool calls and diffs now read as
  Windows paths, so they open.
- Node: setting up a call to a disconnected machine times out after one
  15-second wait and explains that the request was not sent.
- Node: Linux and Windows packages build successfully for releases.
- Node: delegated agents and their status appear in flsh sessions when the harness reports them.
- Node: an agent that starts a helper agent is now told to stop the helper
  with `stop_agent` once it no longer needs it. Finished helpers used to stay
  open and listed as running until the agent that started them ended.
- Node: device streaming paces frames consistently and keeps touch input responsive when video writes are blocked.
- Node: Codex sessions show the questions Codex asks while it keeps
  working, and send your answer back to it. The node updates the Codex
  adapter it installed to a version that forwards them, and reinstalls an
  installed adapter that is too old.
- Node: a Codex question that takes your own words no longer asks for them
  again as a separate "Additional answer or note" question.
- Node: a coding agent session started in a terminal outside Beep now
  shows as ended as soon as its process exits, so you can continue it in
  Beep right away. Before, it showed as running outside Beep for up to
  three minutes.
- Node: on Wi-Fi networks that rewrite DNS answers, such as some hotel
  content filters, the node could not reach any machine. Names now resolve
  through encrypted DNS first, then the network's DNS if that fails.
- Node: changes made by a session working in a linked worktree inside a
  project's .worktrees folder now reach views that follow all session
  folders. Before, those changes were dropped.
- Node: Codex sessions show the questions Codex asks while it keeps
  working, and send your answer back to it. The node updates the Codex
  adapter it installed to a version that forwards them, and reinstalls an
  installed adapter that is too old.
- Node: a Codex question that takes your own words no longer asks for them
  again as a separate "Additional answer or note" question.
- Node: a coding agent session started in a terminal outside Beep now
  shows as ended as soon as its process exits, so you can continue it in
  Beep right away. Before, it showed as running outside Beep for up to
  three minutes.
- Node: coding agent updates grant Homebrew trust only to the installed formula when required, then retry the upgrade instead of hiding the failure behind the agent updater.
- Node: image attachments no longer become conversation titles when a session has no name.
- Node: a Claude Code turn that finished normally no longer ends with
  "The turn did not finish" when the same step asked for the advisor
  next to a local command. The advisor request never ran, so that turn
  now ends normally.
- Node: "Stop all" now ends a session's subagents even after the turn was interrupted first, instead of only cancelling the turn.
- Node: detect when an agent moves into a linked Git worktree.
- Node: A newly started assistant session says it is the assistant from the
  start, so desktops and phones show it on the Assistant page.
- Node: a message one agent session sends another with `send_to_agent`
  reaches a working session at its next step, as a steer, instead of
  waiting for the turn to end. Pressing Stop on that session no longer
  deletes these messages. They go out as the next prompt.
- Node: after a refused join on a removed node, running `beep-node mesh
  join` again no longer fails with "this directory already belongs to a
  mesh".
- Node: a session past its compaction limit compacts as soon as its turn
  ends, so the next message no longer waits behind a compaction that starts
  only when the message is sent.
- Node: a message sent to a session that has ended opens the session
  again and goes to it, instead of failing with "session is no longer on
  the machine". This covers messages held while offline and sent later.
- Node: a skill library whose skills sit in an agent's folder, such as
  `.claude/skills` or `.cursor/skills`, now lists those skills and they can
  be installed. Before, the library showed 0 skills.
- Node: in a Claude Code session run by a node, clicking the card of a
  subagent started with the default agent opens that subagent's
  conversation. Before, the click only expanded the card's result.
- Node: cancelling a session move after the source machine had already
  stopped the session now starts it again there, instead of leaving it
  ended.
- Node: a temporary code reviewer, started by the review tool or by
  Review with agent, no longer appears in the Sessions list or in
  Home's Done today lane. Its session is removed when the review ends.
- Node: an agent that offers more than 500 models, such as flsh with several
  providers set up, shows all of them in the model picker instead of
  dropping the providers listed last.
- Node: the first Git copy of a project into a folder that already has
  files no longer fails forever with "rebuilding the git index failed" when
  a file differs from the commit. The differing file stays as a local
  change.
- Node: Claude Code sessions run through a node can ask you questions now.
  Claude Code's questions and forms from MCP servers appear in the session
  one at a time, each with its options and a Skip option.
- Node: while a question waits, words typed in the message box become the
  answer. In a multiple choice question, pick several options by typing
  their numbers, such as 1, 3.
- Linux (including WSL 1): installing Node.js for a coding agent's adapter,
  and the gh, teacli and flsh tools, no longer depends on the system `tar`.
  That `tar` fails on Ubuntu 24.04 under WSL 1, so these installs failed
  there.
- Node: Claude Code can be installed from Settings > Coding agents on a machine
  without Node.js, Windows included. Beep uses Claude Code's own installer when
  npm is missing.
- Node: on Windows, installs and updates find the npm that Beep set up in its
  own folder.
- Node: when a desktop does not run commands, the message names the "Let Beep use this computer" setting by its current name.
- Node: after you stop a turn and send a message, the message goes once the
  turn has stopped. Before, it went into the stopping turn, so the agent kept
  working and the session stayed on Stopping.
- Node: a foldable's view switches screens on the next frame after it opens
  or closes.
- Node: an Android phone with two screens, such as a foldable, shows the screen that is on in the Device tab instead of a blank view, and follows the phone as it opens or closes.

### Performance

- Node: builds compile fewer dependencies.
- Node: session lists load faster by reading known checkout folders directly.
- Node: Git sync between machines sends only the commits the other machine
  is missing, instead of the whole history for each new branch. A repository
  that grew to tens of gigabytes from repeated copies stays its normal size.
- Node: only the default branch and the branch checked out on each machine
  sync. Agent worktree branches stay on the machine that made them. A branch
  that both machines have fast-forwards while it is checked out on the
  machine that made the commits.
- Node: desktops and phones search that index, so they no longer download
  transcripts to build their own.
- Node: desktop source builds leave out unused port mapping while standalone node connectivity stays unchanged.
- Node: on Linux, the file watch skips build output and caches
  (target, node_modules, .venv, __pycache__, .next and similar) and
  linked worktrees under .worktrees. Opening the Files tab on a large
  checkout starts watching much faster and uses far fewer inotify
  watches, so the desktop no longer reports that the session
  subscription did not answer in time.
- Node: a folder is watched once however many views follow it, and
  stays watched for five minutes after the last one closes, so
  reopening a project does not set up its file watch again.
- Node: source builds compile fewer dependencies while retaining device control and Git sync.
- Node: Git sync and the branch views keep each repository open between
  reads instead of loading its pack indexes again on every scan, which uses
  less CPU and battery.

## [0.7.22] - 2026-11-17

### Performance

- Node: builds compile fewer dependencies.

## [0.7.21] - 2026-11-17

### Fixed

- Node: resumed Claude sessions recover available earlier messages from native history.

### Performance

- Node: session lists load faster by reading known checkout folders directly.

## [0.7.20] - 2026-11-17

### Added

- Node: on a Windows machine, Codex, Gemini CLI, pi, flsh, Hermes and
  OpenCode installed in a WSL distribution now run as that machine's
  agents when Windows lacks them, as Claude Code already did. They need
  their ACP adapter or npx installed inside WSL.
- Node: sessions that coding agents ran inside WSL now show in Beep's
  session lists and transcripts, under their Windows folders.
- Node: keeps a search index of the prompts and agent replies in the
  sessions of its project folders. It reads the agents' own files on that
  machine.

### Changed

- Node: desktop controllers without hosting remain mesh controllers and no longer appear as coding agent or file hosts, or as phones.
- Node: agents share Cargo build artifacts across linked worktrees.
- Node: a Windows node no longer installs or runs beep-node inside WSL.
  WSL machines added earlier stop running and can be removed from Machines.

### Fixed

- Node: an interrupted Git transfer no longer leaves half-written pack files
  in `.git`. A stalled transfer gives up after 30 seconds of silence instead
  of waiting five minutes. Git packs its objects together once many have
  arrived.
- Node: a machine with a very long system journal answers status checks
  at once, so beep_status no longer shows it as not answering.
- Node: a resumed session shows its added and removed line counts
  right away after a restart, not after its next turn ends.
- Node: file paths in a WSL session's tool calls and diffs now read as
  Windows paths, so they open.
- Node: setting up a call to a disconnected machine times out after one
  15-second wait and explains that the request was not sent.

### Performance

- Node: Git sync between machines sends only the commits the other machine
  is missing, instead of the whole history for each new branch. A repository
  that grew to tens of gigabytes from repeated copies stays its normal size.
- Node: only the default branch and the branch checked out on each machine
  sync. Agent worktree branches stay on the machine that made them. A branch
  that both machines have fast-forwards while it is checked out on the
  machine that made the commits.
- Node: desktops and phones search that index, so they no longer download
  transcripts to build their own.
- Node: desktop source builds leave out unused port mapping while standalone node connectivity stays unchanged.

## [0.7.19] - 2026-11-17

### Fixed

- Node: Linux and Windows packages build successfully for releases.

## [0.7.18] - 2026-11-17

### Added

- Node: stream supported device video using native H.264 capture, with JPEG fallback.

### Changed

- Node: Agents are instructed to use image content and the request when titling sessions, while preserving explicit titles.
- Node: when a coding agent session moves to another machine, the note the
  agent gets now names the machine it came from and the machine it runs on
  now. Before, it only named the old and new folders.
- Node: an agent on a node can now read every machine's status, logs and
  metrics and include them in a problem report, as an agent on a desktop
  can. Before, a node refused these requests from another node, so the
  beep_logs, beep_status, beep_metrics and report tools only covered that
  node. Machines still on an older node release keep refusing until they
  update.

### Fixed

- Node: delegated agents and their status appear in flsh sessions when the harness reports them.
- Node: an agent that starts a helper agent is now told to stop the helper
  with `stop_agent` once it no longer needs it. Finished helpers used to stay
  open and listed as running until the agent that started them ended.
- Node: device streaming paces frames consistently and keeps touch input responsive when video writes are blocked.
- Node: Codex sessions show the questions Codex asks while it keeps
  working, and send your answer back to it. The node updates the Codex
  adapter it installed to a version that forwards them, and reinstalls an
  installed adapter that is too old.
- Node: a Codex question that takes your own words no longer asks for them
  again as a separate "Additional answer or note" question.
- Node: a coding agent session started in a terminal outside Beep now
  shows as ended as soon as its process exits, so you can continue it in
  Beep right away. Before, it showed as running outside Beep for up to
  three minutes.

## [0.7.17] - 2026-11-17

### Added

- Node: serves port forwards to the user's desktops. A desktop can reach
  only ports on the node's own loopback, and only while the machine is
  shared with other devices.
- Node: a server that a session's command starts on a port after the
  session launched now appears in the desktop's Running tab within about a
  minute, because the node checks its sessions' ports once a minute.
- Node: reports which large file a sync waits on and how much of it has
  arrived, so desktops can show it.
- Node: When a coding session's turn ends, the node counts the lines
  added and removed in its checkout, as the Changes tab does, and sends
  them with the session so the desktop can show them beside it.

### Changed

- Node: when a coding agent session moves to another machine, the note the
  agent gets now names the machine it came from and the machine it runs on
  now. Before, it only named the old and new folders.

### Fixed

- Node: on Wi-Fi networks that rewrite DNS answers, such as some hotel
  content filters, the node could not reach any machine. Names now resolve
  through encrypted DNS first, then the network's DNS if that fails.
- Node: changes made by a session working in a linked worktree inside a
  project's .worktrees folder now reach views that follow all session
  folders. Before, those changes were dropped.
- Node: Codex sessions show the questions Codex asks while it keeps
  working, and send your answer back to it. The node updates the Codex
  adapter it installed to a version that forwards them, and reinstalls an
  installed adapter that is too old.
- Node: a Codex question that takes your own words no longer asks for them
  again as a separate "Additional answer or note" question.
- Node: a coding agent session started in a terminal outside Beep now
  shows as ended as soon as its process exits, so you can continue it in
  Beep right away. Before, it showed as running outside Beep for up to
  three minutes.
- Node: coding agent updates grant Homebrew trust only to the installed formula when required, then retry the upgrade instead of hiding the failure behind the agent updater.
- Node: image attachments no longer become conversation titles when a session has no name.

### Performance

- Node: on Linux, the file watch skips build output and caches
  (target, node_modules, .venv, __pycache__, .next and similar) and
  linked worktrees under .worktrees. Opening the Files tab on a large
  checkout starts watching much faster and uses far fewer inotify
  watches, so the desktop no longer reports that the session
  subscription did not answer in time.
- Node: a folder is watched once however many views follow it, and
  stays watched for five minutes after the last one closes, so
  reopening a project does not set up its file watch again.
- Node: source builds compile fewer dependencies while retaining device control and Git sync.
- Node: Git sync and the branch views keep each repository open between
  reads instead of loading its pack indexes again on every scan, which uses
  less CPU and battery.

## [0.7.16] - 2026-11-16

### Added

- Node: Codex fast mode and all available thinking levels.

### Changed

- Node: `beep-node mesh join` accepts the short `beepjoin3` ticket. The
  older `beepjoin2` ticket still works, but desktops no longer make it.
- Node: Beep now keeps every project it excludes from Syncthing in one `// Beep project files` block in the shared folder's ignore file, not one block per project. Old blocks merge on the next change.
- Node: session history metadata is available in pages.

### Fixed

- Node: a Claude Code turn that finished normally no longer ends with
  "The turn did not finish" when the same step asked for the advisor
  next to a local command. The advisor request never ran, so that turn
  now ends normally.
- Node: "Stop all" now ends a session's subagents even after the turn was interrupted first, instead of only cancelling the turn.
- Node: detect when an agent moves into a linked Git worktree.

## [0.7.15] - 2026-11-16

### Changed

- Node: `beep-node mesh join` with a fresh ticket brings a removed node
  back into the mesh.

### Fixed

- Node: A newly started assistant session says it is the assistant from the
  start, so desktops and phones show it on the Assistant page.
- Node: a message one agent session sends another with `send_to_agent`
  reaches a working session at its next step, as a steer, instead of
  waiting for the turn to end. Pressing Stop on that session no longer
  deletes these messages. They go out as the next prompt.
- Node: after a refused join on a removed node, running `beep-node mesh
  join` again no longer fails with "this directory already belongs to a
  mesh".
- Node: a session past its compaction limit compacts as soon as its turn
  ends, so the next message no longer waits behind a compaction that starts
  only when the message is sent.

## [0.7.14] - 2026-11-16

### Added

- Node: beep-node can replace a text file in a working folder when the
  desktop's editor saves it, keeping the file's permissions. Older nodes
  refuse this, so update the node to save edits.

### Fixed

- Node: a message sent to a session that has ended opens the session
  again and goes to it, instead of failing with "session is no longer on
  the machine". This covers messages held while offline and sent later.

## [0.7.13] - 2026-11-16

### Added

- Node: assistant chats get instructions for being the Beep assistant and
  project tools: `projects_hosts` lists Git hosts, `projects_create` creates
  a project (private repository unless asked), `projects_clone` clones a URL,
  `projects_found` lists the projects found on your machines and
  `projects_import` imports them.
- Node: the project tools need a desktop to be online. Agents that the
  assistant starts do not get them.
- Node: an assistant chat stays one after beep-node restarts. A chat moved
  to another machine keeps its title, model and thinking level.
- Node: on a Windows machine without Claude Code installed on Windows,
  the node now runs the Claude Code installed in a WSL distribution as
  that machine's Claude Code. Sessions work on the project's Windows
  folder, seen from WSL under `/mnt/`. The machine's Claude Code sign-in
  is passed into WSL, and Beep tools work there. The WSL distribution no
  longer needs to be added as a separate machine.
- Node: with a Meshy API key saved, coding agents can animate, retexture,
  remesh, convert, resize, UV unwrap, split and prepare 3D models for
  printing, and make reliefs and Meshy images.

### Fixed

- Node: a skill library whose skills sit in an agent's folder, such as
  `.claude/skills` or `.cursor/skills`, now lists those skills and they can
  be installed. Before, the library showed 0 skills.
- Node: in a Claude Code session run by a node, clicking the card of a
  subagent started with the default agent opens that subagent's
  conversation. Before, the click only expanded the card's result.

## [0.7.12] - 2026-11-15

### Added

- Node: moving a session reports to the desktop when the project folder on
  the source machine has uncommitted edits. Those edits stay on the source
  machine. Only the session and the worktrees it made move.

### Changed

- Node: moving a session no longer waits for every file of the project.
  The target waits until its checkout is at the same commit as the
  source's.
- Node: the worktrees a session created move with it, including their
  branch, commits and uncommitted changes. Other worktrees stay where
  they are. The source keeps its copy of a moved worktree, detached from
  the branch.

### Fixed

- Node: cancelling a session move after the source machine had already
  stopped the session now starts it again there, instead of leaving it
  ended.

## [0.7.11] - 2026-11-15

### Added

- Node: a session's plan steps carry when they started and ended, so the
  apps can show each step's subagents.

### Changed

- Node: a Claude Code session shows its task list as the session's plan
  again. Claude Code had its task tools off on recent models.

### Fixed

- Node: a temporary code reviewer, started by the review tool or by
  Review with agent, no longer appears in the Sessions list or in
  Home's Done today lane. Its session is removed when the review ends.
- Node: an agent that offers more than 500 models, such as flsh with several
  providers set up, shows all of them in the model picker instead of
  dropping the providers listed last.

## [0.7.10] - 2026-11-15

### Changed

- Node: with Project sync on, adds the Syncthing ignore rule for each
  project in one of its shares unless the project chose Syncthing.
- Node: a node gives a problem report its whole log for the chosen period,
  instead of the newest 1 MiB.
- Node: a problem report can be up to 95 MiB, up from 5 MiB, and a large
  one gets more time to upload.

### Fixed

- Node: the first Git copy of a project into a folder that already has
  files no longer fails forever with "rebuilding the git index failed" when
  a file differs from the commit. The differing file stays as a local
  change.

## [0.7.9] - 2026-11-15

### Added

- Node: Claude Code can run directly on the `claude` CLI instead of the npm
  ACP adapter, when its harness is set to Direct. Prompts, streamed text and
  thinking, tool calls, tool permission prompts, plan approval, Claude's
  questions (shown as forms when the app supports them), model and mode
  changes, stopping a turn, and resuming a session with its history all work.
  Claude Code also runs on a machine that has its CLI but neither its ACP
  adapter nor Node.js/npx, through the node's own Direct driver.

## [0.7.8] - 2026-11-15

### Added

- Node: coding agents can rig a textured humanoid 3D model so it can be animated, optionally remeshing it and setting its height. The `model_3d` tool's new `rig` action needs a saved Meshy API key, takes a GLB from the session folder or an HTTP(S) URL, and saves the rigged GLB in the session's working folder.
- Node: a Mac node lists the iPhones and iPads plugged into it, so they
  appear in the Device tab and to agents. The screen, app installs and
  app launches work as on the desktop. Taps, typing and buttons are not
  available yet.

### Changed

- Node: the Beep tools for coding agents now have one tool per action, such as `list_machines`, `remote_run`, `remote_run_status`, `send_files`, `start_agent`, `device_tap`, `beep_logs`, `generate_image`, `generate_3d`, `rig_3d` and `list_models`. Each tool takes only its own arguments, so agents mix up arguments less often.
- Node: the `plugins` tool is removed, so coding agents can no longer list, create or call Beep plugins.

### Fixed

- Node: Claude Code sessions run through a node can ask you questions now.
  Claude Code's questions and forms from MCP servers appear in the session
  one at a time, each with its options and a Skip option.
- Node: while a question waits, words typed in the message box become the
  answer. In a multiple choice question, pick several options by typing
  their numbers, such as 1, 3.

## [0.7.7] - 2026-11-15

### Changed

- beep-node: the log says where a failed incoming connection came from:
  its address, or its endpoint id when it came through a relay. Warnings
  for an incoming connection name its protocol and machine.

## [0.7.6] - 2026-11-15

### Added

- Windows: the node runs the WSL machines it added only while Beep may use
  the computer. It leaves one alone when another Beep on the PC already
  runs it. If you stop WSL, for example with `wsl --shutdown`, the node
  waits until the machine is turned on again or the node restarts. A
  distribution removed from WSL is forgotten.
- WSL: a node running in WSL is named after its distribution and the
  Windows PC, such as Ubuntu on gpu-box.

### Fixed

- Linux (including WSL 1): installing Node.js for a coding agent's adapter,
  and the gh, teacli and flsh tools, no longer depends on the system `tar`.
  That `tar` fails on Ubuntu 24.04 under WSL 1, so these installs failed
  there.
- Node: Claude Code can be installed from Settings > Coding agents on a machine
  without Node.js, Windows included. Beep uses Claude Code's own installer when
  npm is missing.
- Node: on Windows, installs and updates find the npm that Beep set up in its
  own folder.
- Node: when a desktop does not run commands, the message names the "Let Beep use this computer" setting by its current name.

## [0.7.5] - 2026-11-14

### Added

- Node: when an agent sends files to another machine, the sending node reports
  how many bytes the receiving machine has fetched. The agent session shows
  this as progress on the running `files` tool call, updated about once a
  second.

### Fixed

- Node: after you stop a turn and send a message, the message goes once the
  turn has stopped. Before, it went into the stopping turn, so the agent kept
  working and the session stayed on Stopping.

## [0.7.4] - 2026-11-14

### Added

- Node: agents read a device's newest log lines with the `device` tool's
  `logs` action: logcat for Android phones and emulators, the last five
  minutes of an iOS simulator's log, with an optional filter and line count.
- Node: the `device` tool's `ui` action lists what the screen shows, each
  element's label, id and where to tap it, for Android devices and iOS
  simulators.
- Node: the `device` tool installs an APK or `.app` already on the device's
  machine (`install`), starts an app or opens a link (`launch`), and stops an
  Android app and clears its data (`reset`).

## [0.7.3] - 2026-11-14

### Added

- Node: desktops can follow the machine's phones and hear when one connects.

### Fixed

- Node: a foldable's view switches screens on the next frame after it opens
  or closes.

## [0.7.2] - 2026-11-14

### Fixed

- Node: an Android phone with two screens, such as a foldable, shows the screen that is on in the Device tab instead of a blank view, and follows the phone as it opens or closes.

## [0.7.1] - 2026-11-14

### Added

- Node: when a coding agent sends a Beep problem report, the approval
  offers "Send and open issue". It also opens a public GitHub issue
  with your signed-in `gh` account. The issue carries the description,
  the report reference and the versions, but no logs. The agent gets
  the issue link back.
- Node: coding agents read Beep's status and logs first, and offer a
  problem report when Beep itself fails.
- Node: an Android phone connected to the machine over adb, by USB or
  wireless debugging once debugging is allowed on the phone, is listed with
  the machine's devices. The Device tab and agents' `device` tool show its
  screen and tap, swipe, type and press keys on it. A phone cannot be booted
  or shut down from Beep.
- Node: the machine tells desktops when adb sees a phone or emulator connect
  or disconnect, so the device list updates without waiting for a refresh.

## [0.7.0] - 2026-11-14

### Added

- Node: agents on every machine get a `plugins` tool. On a machine without
  the Beep desktop, the call goes to an online desktop in the mesh. It fails
  with a clear message when no desktop is online.

### Changed

- Node: running session processes report the process that started them and a longer command line, so the desktop can group them and show the command inside an agent's shell wrapper.

## [0.6.33] - 2026-11-14

### Changed

- beep-node: A session's temporary attachments are deleted one day after
  the session ends, unless the session runs again.
- beep-node: Any attachment, including files dropped on a new agent's
  prompt into a project's `.beep/uploads` folder, is deleted after three
  days without being read or written.

## [0.6.32] - 2026-11-14

### Added

- Node: a session remembers the worktrees it created, both the one Beep
  made when it started and the ones the agent added with
  `git worktree add`. The list survives node restarts and goes with the
  session when it moves to another machine.

## [0.6.31] - 2026-11-14

### Added

- Node: an agent can start another agent in the background. The
  background agent is deleted when the agent that started it closes. It
  does not show on the project page.

### Changed

- Node: a session moved to this machine resumes in the project folder,
  and the node no longer makes a worktree for it. Committed branches
  arrive through Git. Uncommitted work in a worktree stays on the
  machine it was on.
- Node: a coding agent sign-in started from another machine now writes a
  line to the node's log at each step: started, link offered, code
  received, and how it ended. The log never holds the link, the code or
  the agent's output.

### Fixed

- Node: agents working in a worktree name files by their full path from the session folder, so links in their replies open in the file pane.

### Performance

- Node: a project's branch list and each branch's upstream, ahead and
  behind counts are read directly from Git instead of starting a Git process
  for every branch whenever refs change. Branches refresh faster and use less
  battery.

## [0.6.30] - 2026-11-14

### Changed

- beep-node: agent sessions always run in the project folder, which is
  the repository's main checkout. A session asked to work in a worktree
  runs in the project and is told to work in that worktree. A session
  may use several worktrees and branches. Once its work merges, the
  session is told to remove every worktree it created.

### Fixed

- beep-node: a session whose worktree was removed resumes in its project
  folder after beep-node restarts, instead of failing to resume.
- Node: a session goes on with the model it last ran on when it is
  taken over again after the node restarted, or after the node stopped
  without a clean shutdown, instead of the agent's default model.

## [0.6.29] - 2026-11-14

### Added

- Node: a session keeps the emulator or simulator its agent last used with the `device` tool, also across a node restart.
- Node: `agent list` takes a project name or `all` and shows every running
  agent on every machine, not only the ones you started. Agents in the same
  project can find each other whatever harness they run.
- Node: `agent send` messages any agent from that list. The receiver gets the
  message as a new turn and can reply with `agent send`. An agent takes at
  most five messages from other agents between two messages from you, so
  agents cannot keep each other talking.

### Changed

- Node: agents are told how to install and start an app on an emulator or
  simulator, to reuse a device that is already on, and not to shut down one
  the user may be using.

### Performance

- Node: a synced Git project checks only the branches that moved
  when another machine announces its branches, instead of running Git
  for every branch.
- Node: a file or Git view whose read keeps failing retries less
  often instead of up to four times a second.
- Node: checking whether a path lies in a session's worktrees runs
  Git again only after worktrees are added or removed.
- Node: idle agent sessions share one process list read about once
  a minute instead of each reading its own.
- Node: Git sync and the project and session views read branches,
  commits and repository folders directly instead of starting a Git
  process for each question, which uses less battery.

## [0.6.28] - 2026-11-14

### Added

- Node: tells the desktops when a review agent has handed in its review.

### Fixed

- Node: Claude sign-in detection on macOS now follows Claude's auth status instead of treating leftover Keychain entries as a sign-in.

## [0.6.27] - 2026-11-14

### Added

- Node: ACP v2 support is back but off by default; set BEEP_ACP_VERSION=2 in
  an agent's environment to try it (it falls back to v1 when the agent only
  speaks v1). Agents with v2 now accept the Beep MCP server instead of failing
  to start.

### Fixed

- Node: an agent's message is no longer cut in two when a background
  subagent calls a tool while the message is being written.

## [0.6.26] - 2026-11-13

### Fixed

- Node: Claude Code and Codex chats start again. The ACP v2 support added
  in the previous release made them fail with "data did not match any
  variant of untagged enum McpServer", so it was rolled back to ACP v1.

### Performance

- Node: reduced background work when agent sessions update.

## [0.6.25] - 2026-11-13

### Added

- Node: prefer ACP v2 for agents, fall back to v1 when an agent selects it, and allow `BEEP_ACP_VERSION=1` in the harness environment to select v1.

### Fixed

- Node: avoid duplicate ACP v2 subagent steps, keep idle sessions inactive after failed compaction, and retain model, effort and mode controls after incomplete configuration replies.
- Node: interruption notices appear as status instead of becoming part of the next user prompt.

## [0.6.24] - 2026-11-13

### Changed

- Node: child agent completions no longer send notifications; results still reach the parent agent.

### Fixed

- Node: recovery instructions appear as Beep context, and session titles survive a node restart.
- Node: image prompts sent while a resumed agent starts no longer need to be retried.
- Node: When an API key is saved or removed while a coding agent runs, the agent now sees the image, video, speech, model_3d and decision tools appear or go away without starting a new session.
- Node: a folder that becomes a Git repository while it is being watched shows
  its Git state and later commits.
- Node: a project whose handover to Syncthing fails is retried instead of
  disappearing from sync.
- Node: after a project is handed to Syncthing, files Syncthing changed are
  not treated as local edits or deletions when Beep sync resumes.
- Node: with four devices attached, a fifth detaches the least recently used
  one, and shutting a device down by name or id detaches its viewer.
- Node: prompts sent while answering an agent's question count against the
  queue limit.
- Node: an alert is no longer skipped when many arrive at once.
- Node: a change in flsh credits is announced to connected apps.
- Node: worktrees are listed correctly when Git runs in another language.
- Node: the flsh update notice names version 0.2.9.
- Node: on Windows, the machine reports its real memory.
- Node: a host name longer than 64 bytes is shortened instead of shown as
  "Machine".
- Node: watching many folders no longer interrupts the connection to the
  desktop.

## [0.6.23] - 2026-11-13

### Added

- Node: The `model_3d` generate action takes `polycount`, the most faces the model may have, for Meshy and Tripo. A lower count gives a smaller file that loads faster. Credits charged stay the same. Other provider options pass through to Meshy or Tripo.

### Fixed

- Node: The media tools (image, video, speech, model_3d, decision) now send every argument they accept to the provider. An unknown argument, a wrong type, or an argument that belongs to another action is refused with a message naming it. Before, asking Meshy for a low-poly model returned a full-detail one.
- Node: Background commands honor their timeout, file sync rejects unsupported paths and destinations, and MCP tools reject invalid or irrelevant arguments.

## [0.6.22] - 2026-11-13

### Fixed

- Node: automatic session titles are shorter and no longer show internal tags.
- Node: attachments no longer have per-file or total size quotas.

## [0.6.21] - 2026-11-13

### Added

- Node: skill libraries. A node clones each library at the commit the desktop
  picked, then installs the chosen skills into the agents' skills folders and
  into project checkouts. Project skills kept out of Git are listed in
  .git/info/exclude. Unpicked skills are removed. A skill you edited, or one of
  the same name you made, is never overwritten or deleted, and skill sync
  leaves library skills out.
- Node: update matching Git fetch and push URLs after a repository is renamed, including when reconnecting.
- Node: subagent lists show the model when it is known.

### Changed

- Node: Agents see the media tools grouped by kind (`image`, `video`, `speech`, `model_3d`) instead of one tool per operation. Each group has a `models` action that lists the models your providers offer right now, optionally filtered by provider or name. `decision` is unchanged.

### Fixed

- Node: `beep-node update` and `update --now` restart the node on the installed version when it is already the newest release but the running node is older, instead of reporting it as up to date. `--now` restarts at once; otherwise the restart waits until agent sessions finish their turns. `update --check` reports that the node still runs the older version.
- Node: uploaded attachments in project folders no longer appear as files made by the agent.
- Node: listing sessions while a session updated could stop the node, so desktops and phones could no longer reach it.
- Node: flsh installs and updates on Windows x86_64 using the published Windows executable and checksum.

## [0.6.20] - 2026-11-13

### Added

- Node: tells the desktop whether a session's checkout is an Android or iOS
  app, from its project files, so the Device tab shows only for those.
- Node: with an API key saved in Settings, every coding agent (Claude Code,
  Codex, flsh) gets Beep MCP tools for images, video, speech, transcription,
  3D models and decisions. The node runs them with the key, which never
  reaches the agent, and saves what they make in the session's folder.
- Node: coding agents get a `device` tool to list Android emulators and iOS
  simulators on every machine, boot, show or shut one down, take
  screenshots, and tap, swipe, type and press keys and buttons on it, so
  they can test the apps they build. Another machine's devices are reached
  only while that machine is lent to other devices.

### Changed

- Node: coding agents get the Beep tools as a few grouped tools (machines,
  run, files, device, agent, review, ask_user, beep) that take an action,
  using about half the context. Run and files only appear when there is
  another machine.

### Fixed

- beep-node: sync progress counts bytes as a file arrives, so a large file
  no longer sits at 0% until it is done.

## [0.6.19] - 2026-11-13

### Changed

- Node: an agent that takes no images gets a message's text without them, rather
  than the message being refused.

## [0.6.18] - 2026-11-13

### Fixed

- beep-node: sync can start over in a project folder that was replaced, scanning
  it from nothing so nothing is deleted on other machines.

## [0.6.17] - 2026-11-13

### Added

- Node: API keys saved in Settings reach flsh and other processes as env
  vars, such as `MESHY_API_KEY` and `ELEVENLABS_API_KEY`.
- Node: serves its Android emulators and iOS simulators to desktops, live,
  with touch, typing, buttons and rotation. iOS simulators need
  idb-companion (`brew install meta/fb/idb-companion`).

## [0.6.16] - 2026-11-13

### Fixed

- beep-node: problem reports from builds without a configured server go to the
  report receiver instead of the product website.

## [0.6.15] - 2026-11-12

### Added

- Node: installs Node.js into its own folder without an admin password, and
  Git with Homebrew or the Xcode tools on macOS or winget on Windows, when a
  desktop asks during setup.

### Changed

- Node: resuming a Claude Code session whose folder was deleted names that
  folder and says to recreate it.

## [0.6.14] - 2026-11-12

### Added

- Node: serves audio files to the desktop's agent pane, so they play there.

### Fixed

- Node: images a Claude Code subagent took load in its parent session's log
  after the node restarts, instead of staying empty tiles.

## [0.6.13] - 2026-11-12

### Changed

- Node: agents learn that file paths relative to their folder open in the
  desktop's file pane at the line given, and that videos, 3D models and the
  files they write show there too.

### Performance

- Node: desktops and phones fetch media files from chats over iroh-blobs, so
  an unchanged file is not sent again.

## [0.6.12] - 2026-11-12

### Changed

- Desktop: an agent on another machine can send files to a desktop instead of
  being refused.

### Performance

- Node: runs git less often while agents work: a git command no longer makes
  it rewatch every folder and read them all again, file sync reads the
  checked-out commit once per change, and a folder's file list waits longer
  between reads while it stays the same.

## [0.6.11] - 2026-11-12

### Changed

- Node: coding agent skills sync only after a desktop turns skill sync on.

## [0.6.10] - 2026-11-12

### Fixed

- macOS: beep-node starts again after updating itself, instead of staying
  stopped because launchd refused the replaced binary.
- Plan usage shows the weekly limits of single models, such as the Fable only week, next to Opus and Sonnet.
- beep-node: a session resumed after the node restarts goes back to the model
  it ran on, not the agent's default.

### Performance

- Node: an idle node starts far fewer background processes. The machine's
  tool and sign-in check runs once for every status connection and for
  `list_machines`, path checks no longer run `git` for each removed
  worktree, and idle session process and open-file checks run less often.
- The search for checkouts skips media, app and cache folders such as Pictures, Music, Movies and Library, so it is quicker and macOS does not ask for photo or music access.

## [0.6.9] - 2026-11-12

### Changed

- Node: sessions from other machines are no longer copied in the
  background. A session is copied when you continue it on another machine,
  and both machines must be online. Copies left from earlier versions are
  deleted on start. Versions of a session that lost to another machine's
  are kept on disk.

## [0.6.8] - 2026-11-12

### Performance

- Node: background session copying reads its cache once per pass, stops
  trying sessions once the cache is full, and skips passes while nothing
  changed, instead of rescanning the cache for every session it skips.

## [0.6.7] - 2026-11-12

### Changed

- Node: a Claude Code turn that was cut off reads as an "Interrupted" note instead of a message from you.
- Node: an update from Beep waits up to 30 minutes, instead of 2, for running agent turns to finish before it restarts the node.

### Fixed

- Node: a subagent's Beep tool calls read by their title, such as "Run on <machine>: <command>", instead of the raw tool name.

## [0.6.6] - 2026-11-12

### Fixed

- Node: on a flaky network the node no longer climbs to several cores of CPU and gigabytes of memory; the networking library kept retrying the same connection paths in a queue that never stopped growing.

### Performance

- Node: a project that cannot sync on a machine (uncommitted changes, or
  ahead of the others) no longer keeps machines busy retrying; they wait
  until it changes.
- Node: synced files under 16 KB are no longer copied into the sync database, which kept it large and held much of it in memory.

## [0.6.5] - 2026-11-12

### Changed

- Node: agents name machines without apostrophes or symbols, so
  "Alex's Mac Studio" is `alexs-mac-studio` instead of `alex's-mac-studio`.

### Fixed

- Node: a connection to a machine that stops answering is dropped and dialed again after about 15 seconds instead of 30.
- Node: on a laptop that sleeps, the node reconnects to the other machines within seconds of waking instead of up to a minute later.

### Performance

- Node: looking up one session started outside Beep reads only that session's file instead of every Claude Code transcript on the machine, and several are answered in one request.
- Node: lookups that older desktops keep live wait longer between reads when nothing changed, so an older desktop no longer keeps a node busy.
- Node: a request that ends well within a second no longer writes four lines to the log; only slow, failed or dropped requests are logged.

## [0.6.4] - 2026-11-11

### Performance

- Node: listing the checkouts sessions worked in reads each checkout with git
  only after a commit, branch switch or remote change in it, skips git for
  folders outside any checkout, and shares one listing between requests that
  arrive together. A desktop up to 0.8.3 that keeps this list open gets it at
  most once a minute instead of after every session update.

## [0.6.3] - 2026-11-11

### Added

- Node: a desktop can close a session running in a terminal so it continues
  in Beep. The node ends only the process it found running that session,
  with SIGTERM on Linux and macOS; on Windows the process is ended at once.

### Performance

- Node: checks of which process holds a Codex or flsh session's file are
  shared by every list and open session view, kept until the file is written
  or its process exits, and run for many files at once. Finding Claude Code
  sessions reuses the node's process list and reads each process's folder
  once. This cuts the `lsof` and `ps` processes started on macOS.

## [0.6.2] - 2026-11-11

### Fixed

- Node: Codex installed with its own installer shows its updates and
  updates with `codex update`.

## [0.6.1] - 2026-11-11

### Performance

- Node: while a Claude Code, Codex or flsh session runs outside Beep,
  the lists of such sessions are read again at most every 2 seconds instead
  of 4 times a second each, so the machine scans its agents' files far less.

## [0.6.0] - 2026-11-11

### Added

- Release pages include versioned release notes.
- The node metrics log counts started programs, file watch events, sync scans, topic changes, snapshot reads and state file writes.

### Changed

- Node: flsh models get readable names, such as "DeepSeek V4.1 Flash 1017"
  for `openrouter:deepseek/deepseek-v4.1-flash-1017`, with the provider
  reported apart from the name.

### Fixed

- Node: listing one agent's models no longer waits for another agent's
  slow listing, so Hermes and flsh answer the start form promptly.
- Node: Stop ends only the main agent's turn. Its subagents, background
  commands and the agents it started keep running, and the session shows
  as working until they finish.
- Node: a steering message reaches the agent at its next step while
  background commands and subagents run, instead of waiting for them.
- Node: a Claude Code skill that runs in its own subagent shows as
  "Skill: <name> (subagent)" in the session log, so its steps no longer
  look like a plain skill load. Loading an ordinary skill stays a normal step.
- A session whose turn ended while its subagents still run reports them, so
  apps show it as working; node restarts and session moves wait for them.
- The "turn finished" notification waits until the session's subagents end
  too, or until they send nothing for 30 minutes. Questions still notify at once.
- Node: a session's log keeps Beep's whole agent brief as context instead of cutting off its last sentences.
- Node: a project waiting on Git keeps its warning instead of flickering between scanning, synced and failed on every scan.
- Node: the worktree menu leaves out worktrees of other machines listed in a synced `.git` folder, and sessions keep their folder when an agent runs a command on another machine.
- Node: a session whose background subagent finished, or whose turn was stopped mid-subagent, stops showing as working and sends its "turn finished" notification right away.
- Node: more session views can stay open across desktop windows and phones at once (128 instead of 32); idle views drop their cached transcript when memory runs short.
- Node: closing a session stops dev servers its commands left running, even when a wrapper started them and exited at once.
- Node: Git branch and history views refresh for a folder that became a repository, or whose Git lookup failed once, after its next file change.
- Node: a node started with its own `--dir` keeps its session presence file in that folder, so it no longer replaces or removes the installed node's.
- Node: an action from a newer desktop or phone that this node cannot read
  yet gets an error back, instead of dropping every live view on that link.
- Node: Stop all also stops the background commands and subagents of agents
  the session started on other machines, not only their turns.
- Node: a Claude Code transcript replaced by a new file is always read again
  in full, even when its size and time match the old one.
- Node: after a sleep or a Wi-Fi drop, mesh links to other machines are
  dialed again at once.

### Performance

- The node uses less CPU while agents work: live views of sessions, processes, terminals and folders refresh at most a few times a second, and one session's view no longer lists every session.
- On macOS, idle agent sessions no longer run `ps` every second when no app shows their processes.
- Git branch, history and upstream views refresh only when Git refs, the index or settings change, not on every file save or Git object write.
- Builds and checkouts in synced projects no longer flood listings with one change signal per file.
- Streaming agent sessions open in an app cost the node less CPU: each update encodes only new or changed transcript entries.
- Node: an idle machine wakes far less often. Mesh links ping every 5
  seconds instead of 3, the health check runs every 15 seconds instead of 5,
  the timer lag probe runs with the minute's metrics instead of every second,
  and machine reports, the CI watch, skill checks, session copies and mesh
  settings run only when something changed or much less often.
- Node: a mesh link rides out a network drop of up to 30 seconds instead of
  reconnecting after 10, and a machine that stays offline is dialed about twice
  a minute instead of every 12 seconds. One that comes back is reached at once.
- Node: conversations open faster. A newly opened session sends its newest
  page first, a Claude Code transcript parses only the lines added since the
  last read, its subagents' files are read again only when they change, and a
  followed session looks up its file once and its processes only when they
  may have changed.
- Node: following a Claude Code session started outside Beep takes far less
  CPU: about 1% of a core instead of 17 to 50% for a session writing 20 lines
  a second. Each update parses only the newly written lines
  for the session's title and last prompt, reads only the entries the app
  does not already hold, reads the process list at most every 10 seconds
  when no process runs the session, and waits 200 ms to batch writes. Chats
  Beep runs still update at once.
- Node: following a Claude Code session lists its project folder only when a
  file comes or goes there, not on every update.

