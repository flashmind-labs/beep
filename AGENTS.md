# Agent notes

## Getting the app onto the phone

The Android workflow (`.forgejo/workflows/android.yml`) builds the signed
bundle and uploads it to Play, and the phone updates from Play:

- Only an `android-v` tag builds and uploads; a manual run of the workflow
  goes to the internal track.
- `just android-release 0.2.3` sets `versionName`, commits and tags
  `android-v0.2.3` (it needs a clean tree on main). Push it with
  `git push origin main android-v0.2.3`.
- A tag goes to the `beta` track, or to the track in the repository variable
  `PLAY_BETA_TRACK`. Nothing goes to production while we are on 0.x. A tag that
  does not match `versionName` fails.
- Releases stay drafts until the repository variable `PLAY_RELEASE_STATUS` is
  `completed`.

iOS works the same way: `just ios-release 0.2.3` sets `MARKETING_VERSION` in
`apps/ios/project.yml`, commits and tags `ios-v0.2.3`. Pushing the tag uploads
it to TestFlight.

To release Android, the desktop app, beep-node and iOS together, run
`just release`. It bumps each app's patch number from its latest tag, runs the
release recipes and pushes main with the tags. Pass versions to pick them:
`just release 0.2.0 0.4.0 0.1.1 0.2.0` (Android, desktop, node, then iOS). The
node and iOS are left out until their first `node-v` or `ios-v` tag exists.

Only release an app when it gets a new version. Do not run a release recipe,
tag or upload a build unless a new version number is being cut for that app.

Do not hand-install APKs on the phone: a locally built APK is signed with
another key and will not install over the Play build. Use the debug APK on an
emulator (`./gradlew assembleDebug`).

## Servers and hosting

Desktops and `beep-node`s form a mesh and work with no server. Every order
(agents, files, terminals, project sync, credentials, usage) goes directly
peer to peer, signed by the device list. A project with a checkout on two machines syncs by itself, in any folder (`files` is not needed), wherever Syncthing does not hold that folder; a held folder starts when Syncthing pauses or lets go, and two checkouts at different commits wait, unchanged, until they agree. A desktop mints tickets and adds
machines; a node can only join, and the server hands out no tickets. Project
sync, groups, pins, credentials and settings are replicated state that only
desktops may write.

`beep-server` is optional and paid: memories, routines, app connections, the
assistant, the project agent and reviews, and the phones (Android and iOS
still talk only to the server). A node linked to a server keeps that link;
once in a mesh it ignores what the server pushes for sync, credentials and
agent settings. A desktop with a server mirrors imports and credential saves
to it.

The server runs no user code. A hosted user's `beep-node` starts with
`--workspace /home/user` (`BEEP_NODE_WORKSPACE`): `exec` and `process` run
there by default, and uploads, downloads and app attachments are saved there.
Per-user nodes are set up by hand for now.

The production server is deployed from the private ops repo, not from
here. App builds pass its `https://` address in `BEEP_SERVER_URL` (a repository
variable in CI, `-PbeepServerUrl` for Gradle); TLS vouches for the server, and its
key comes from the server itself. Ask before deploying to it, and never stop or recreate it
without asking. The on-prem bundle (`deploy/onprem/`) is retired.

Moving an account: credentials are entered again on the desktop. A node in the mesh stops
using the server's credentials and harness values as soon as the account is moved.

### The desktop runs without the server for now

`SERVER_ENABLED` (`apps/desktop/src-tauri/src/beep/mod.rs`) is false: the desktop never contacts `beep-server`, a saved link reads as unlinked and its server features are hidden. Nothing is removed; turning it on brings them back.

Coding agents get no memories: no memory tools and no standing rules. Memories are the assistant's.

`LIVE_ENABLED` (`apps/desktop/src-tauri/src/voice/mod.rs`) is false: the orb window never opens, Option never starts live mode or opens the microphone (two taps still open the bar), and live hints and shortcuts are hidden.

### Code map

`crates/server/src`, one line per top-level module:

- `account`: `User`, single-user instance mode, hosted `login`, guest keys, credentials.
- `agent`: around a turn: models, metering, compaction, gates, titles, sub-agents.
- `api`: request handlers, `routes`, the op `router`, the apps' `http` and session `auth`.
- `app`: the `App` hub: admission, `submit`, the turn itself in `run/turn.rs`.
- `apps`: app catalog, Composio, MCP, `permissions`, `grants`, mail and Slack `feeds`.
- `config`: the TOML config.
- `memory`: memories, skills, profile, starters, watchers, the group leak check.
- `nodes`: node connections (`serve`), linking, per-node requests, the workspace.
- `projects`: project agent: tasks, workers, reviews, imports, `repo_watch` (polls the forges, no webhooks), `git_items`.
- `push`: the notice log, events streams (`events`, `hub`), FCM, APNs.
- `research`: research papers: planning, team tools, PDF.
- `routines`: what wakes the agent: `cron`, `places`, `events`, `digest`, `wake`.
- `store`: Postgres queries, one file per kind of row, `migrations`.
- `tools`: the agent's tools, plus web `search` and `fetch`.
- `voice`: voice notes, live transcription (`stt`), filler lines.

### Code map: shared crates

- `crates/mesh` (`beep-mesh`): the signed device list, tickets, replicated state, and the join and peer connections.
- `crates/projects` (`beep-projects`): project shell commands, parsers and names shared by the server and the desktop.
- `crates/fleet` (`beep-fleet`): the desktop side of projects, credentials, settings and usage over the mesh.

## Alerts

A woken run that calls `notify_user` (a place arrival, account events, a digest) is an
alert: its session gets a `chat_alerts` row and `ListAlerts` lists it. `ListSessions` and
`SearchSessions` still return it, for released phones; `ListChats` and `SearchChats` leave
it out. A message sent in it (any new run) deletes the row, and it is a chat. The desktop shows alerts
in the bell's pane and as a macOS notification, never as a chat opening on its own, and hides
them from its local chat copy. Routines users make (`cron`) are not woken runs and stay chats.
Before this, woken chats stay chats.

The bell also lists what the mesh told this desktop, with no server (`notices/history.rs`, kept
in `notes.json`, the last 200 for 30 days): coding agents finished or asking (Allow and Deny on
the row), and from `notices/watch.rs` sync conflicts, failed session moves and a machine offline
for two minutes.

## Hosted login

A `[login]` config section (`website`, `website_key`) makes the server hosted:
an unknown key is a guest that may call `StartLogin` and `CompleteLogin`
(`crates/server/src/account/login.rs`, contract in issue #196). `GET /v1/server`
says `login: true`, which is how an app with no account tells a hosted server
from one that would make its key an account on its first call. Not with
`[instance]`.

## Deploying the server

The apps' HTTP listens on `http_listen` (default `127.0.0.1:47122`), plain
HTTP/1.1 and HTTP/2. Put Cloudflare in front for TLS: a Tunnel to that
address, or a proxied DNS record to an origin port. Cloudflare's gRPC and
WebSocket settings do not matter, nothing uses them. Cloudflare drops a
connection idle for 100 s; the 30 s heartbeat keeps the events stream open.
Turn off response buffering if a zone has it on. Duplex requests (terminals,
live transcription) send and receive at once; try them through the proxy
before counting on them. The database URL must be the
unpooled Neon endpoint (not `-pooler`), since `LISTEN` needs a session.

Scale-out: notices and update ids already cross instances through Postgres.
What still keeps the server to one instance: runs (a run's stream lives in
the instance that started it), node links and everything sent to a node
(agents, terminals, files), live transcription, and the per-user caches in
`App`. Those need routing by user before a second instance can serve.

Deploys live in the private ops repo. Push to main so CI builds the image,
then deploy from there. `deploy/compose.yaml` is the self-host example
(README, Self-hosting).

We are in beta: every app stays on 0.x until production is ready, minor or
patch bumps only, and the release recipes refuse anything else.

Tag a deployed server with `just server-release 0.4.0` (it tags HEAD
`server-v0.4.0`). To roll back, deploy that tag's image from the ops repo. Migrations are not undone, so an older server must still
run against the newer schema.

## UI changes

Every screen is mocked, one artifact per platform:

- Android: https://claude.ai/artifact/NoyjMpUYmVxmBEGAgQfok2
- iOS: https://claude.ai/artifact/SGKUgUgcBfqMimBBGiAEtq
- Desktop, the tray bar and all screens:
  https://claude.ai/artifact/8BxZSzsXaPqHXkn18rRRSL

Before changing an app's UI, update that platform's mock first so it shows
the new screen or state, then change the app to match.

## App tools

The agent's app tools come from two places:

- Composio toolkits (Gmail, Google Calendar, Outlook, Notion, Linear,
  Todoist, GitHub, Slack, ...), named by Composio slug (`GMAIL_MOVE_TO_TRASH`).
  flashmind calls Composio's v3.1 API; some slugs are missing from v3, so
  check against v3.1.
- Fastmail's remote MCP server, exposed as one flat tool per entry in
  `FASTMAIL_TOOLS` (`crates/server/src/apps/mcp.rs`) and named
  `FASTMAIL_<TOOL>`. The server caches its schemas in `mcp_servers`.

`CATALOG` in `crates/server/src/apps/catalog.rs` lists the tools the agent gets
and which of them act. An acting tool asks first when the app is set to Ask
(`crates/server/src/apps/permissions/`). The approval card shows the call's
arguments, and for IDs it shows the record they name: from an earlier
result in the chat, or read back through the tool in `LOOKUPS`. When adding
an acting tool that takes an ID, add its read tool there.

Check a tool's real arguments before writing code against them:

    just tool-schema GMAIL_MOVE_TO_TRASH FASTMAIL_DELETE_EMAIL
    just tool-schema --list googlecalendar
    just tool-schema --json OUTLOOK_GET_EVENT

It reads the Composio key and database URL from `.dev/android-server.toml`.

## Problem reports

A desktop's report sends a `report/1` request to each machine in the mesh. Each `beep-node` answers with its log for a period, status, version and OS.
Only a desktop reads another machine's report (a node is refused); `beep-node report` sends that node's own.
The app that collects them redacts secrets, zips the answers and posts them, signed, to
`POST /v1/reports` on the server's `http_listen`, which replies with a reference `R-<id>`.
It takes 8 reports at once (more get 503), counts at most 10,000 keys a day, and
trusts `CF-Connecting-IP` only from a loopback, private or link-local peer (a
Tunnel's `cloudflared`, here or on the compose bridge) or Cloudflare's ranges
(`CLOUDFLARE_V4`/`V6` in `reports/route.rs`). Node and desktop builds take the
server from the `BEEP_SERVER_URL` repository variable.

The server stores `reports/<mesh>/<id>.zip` and `<id>.json` (the manifest) in
the private R2 bucket in `[reports.r2]` of its config. `just reports` lists
them, `just report R-<id>` downloads and unzips one to `target/reports/`, and
`just report-solved R-<id>` deletes it. The GitHub issue never holds logs or
names, only the reference.

## Server tests

`just test` starts the local Postgres and runs the server tests. A plain
`cargo test -p beep-server` fails without `BEEP_TEST_DATABASE_URL`.

## The apps over HTTP

Phones, desktops and `beep-probe` reach the server over HTTP
(`crates/proto/src/transport/http.rs`, client in `client.rs`, server in
`crates/server/src/api/http.rs`). Nodes stay on iroh (`beep-node/2`).

- `GET /v1/server`: the server's id and whether it has `[login]`. No auth.
- `POST /v1/session`: the app signs `"beep-session/1\0" || server id || key
  || unix seconds` with its device key and gets a bearer token for an hour.
  A 401 means get a new one; a 403 with `beep-refused: revoked` or
  `not-invited` is final. Every request is admitted again, so a removed key
  is refused at once.
- `POST /v1/<Request>`: a msgpack body, a msgpack reply. Streams answer with
  frames: a 4-byte big-endian length, a tag byte (0 ok, 1 error), msgpack.
  Duplex requests (terminals, live transcription) send untagged frames up the
  request body.
- `GET /v1/events?after=<id>`: `Resume`, `Updated`, then `Notice`s and
  `Updated`s as they happen, and a `Heartbeat` every 30 s. An app that
  shows a notice sends `AckNotice`; a phone that has not after 5 s gets it
  by FCM or APNs.

Notices are rows in `app_notices`, so a stream resumes after its last id
(kept 7 days), and every instance hears them by `LISTEN beep_events`.

## Wire compatibility

Phones update from Play on their own schedule, so the server must keep
answering every released app. The apps' wire is named msgpack: structs are
maps and a reader skips fields it does not know.

- New fields take `#[serde(default)]`. Never rename or retype one, and never
  remove one an app still sends or reads.
- Reply enums and `AppEvent` gain variants only behind a new request, since
  a released app refuses a variant it does not know.
- Request numbers are never reused, and the name in the path never changes.
- `crates/proto/tests/wire/named/` holds one encoded sample per request;
  released apps' bytes must keep decoding. A new request gets its fixture
  with `BEEP_WIRE_BLESS=1 cargo test -p beep-proto compat`; never edit or
  delete one. The numbered directories are the positional era, kept as
  history.

Node and mesh messages (the node link, mesh frames and files, mesh state values,
the terminal host) are compact msgpack with no field names, so their fields are
read by position too. A new field goes at the end with `#[serde(default)]`.
Never insert, reorder, retype or remove one, and never use
`skip_serializing_if` or `to_vec_named` (only the `beep-node/1` link writes
maps, for released nodes). Read with `beep_proto::lenient::from_slice`, which
skips trailing fields a newer sender added; plain `rmp_serde` refuses them.

Exceptions to the field rule:

- Signed types (`Device`, `OpBody` and `Signed` in `crates/mesh`) never gain a
  field in place. A signature is checked over the decoded value encoded again,
  so one extra element makes every stored and relayed signature fail and the
  mesh does not open. To change one, bump `SIGN_DOMAIN`, the mesh ALPNs and
  `MESH_DIR`.
- Readers already in the field that read with plain `rmp_serde` refuse a
  longer message: the terminal host (`terminal::Request`, `Reply`, `Hello`)
  while a 0.2.x host can still be running, and the transfer `Manifest` while a
  released receiver can still be reached. Neither gains a field until then.

ALPNs: `beep-peer/2` and `beep-join/2` (a mesh from before them is in `mesh`,
never read; the device joins again with a new ticket, kept in `mesh2`),
`beep-node/2` (the server also accepts `beep-node/1`, maps, for released nodes;
a node offers both) and `beep-sync/3`. Change a protocol's bytes, bump its name.

## Desktop app

`apps/desktop` is the macOS tray app (Tauri 2, its own Cargo workspace). Run
it with `just desktop-dev`, check it with `just desktop-check`, and build the
DMG with `just desktop-build` (it lands in `target/beep-macos.dmg`; the
installer window comes from `apps/desktop/installer/`). The `desktop` Forgejo
workflow builds the same DMG on the Mac runner. Its key and state live in
`~/Library/Application Support/com.flashmind.beep.desktop/`. Its screens are
in the Desktop mock.

To release it, `just desktop-release 0.2.0` sets the version in
`apps/desktop/src-tauri/Cargo.toml`, commits and tags `desktop-v0.2.0` (or use
`just release` for both apps). Pushing the tag runs the same workflow, which refuses to build without the Developer ID
and notary secrets, then attaches a signed, notarized `Beep-0.2.0.dmg` to the
tag's Forgejo release.

The same run uploads the DMG to the R2 bucket in the `CF_*` secrets, served
at https://releases.flashmind.com/beep/macos/ (`Beep-latest.dmg` is the
fixed download link). Installed apps check `latest.json` there every six
hours, download an update quietly and offer "Restart to Update Beep" in the
tray menu; quitting installs it too. The update signing key is the
`TAURI_SIGNING_PRIVATE_KEY` secret, with its public half in `tauri.conf.json`;
losing it means installed apps can no longer update.

A session's file reads and tree go through `src-tauri/src/work_copy.rs`: when
sync (Syncthing or Beep's) keeps a copy of the project on this Mac they come
from that folder, otherwise from the node. Nothing is compared; sync is trusted.

### Code map

`apps/desktop/src` is grouped by feature. `foo-state.ts` next to `foo.ts`
holds its pure, tested logic. Each window's bundle starts at the entry named
in `build.mjs`.

- `bar/`: the bar's page (entry `app.ts`), its views, keys and live voice.
- `center/`: the command center window (entry `center.ts`): sidebar, Home, the bell and its notifications pane (`notices.ts`).
- `project/`: the Projects page and one project's page.
- `addproject/`: the Add a project, Add a folder, Import a project and New project sheets.
- `review/`: branches, reviews and the diff viewer.
- `agents/`: coding agents: start form, picker, a session's log, the list (`sessions.ts` is the one shared session list), and the empty view of ⌘N (`draft.ts`: a session's layout with the project's start input, which becomes the session in place).
- `agentpane/`: a session's side pane: media, files, changes, artifacts.
- `chat/`: chats, groups, the composer and reply cards.
- `lists/`: Memories, Routines and Skills.
- `machines/`: the Machines list, Home's machine table, Add a machine.
- `terminals/`: terminals kept running on the machines.
- `settings/`: the Settings pages and sheet.
- `research/`: the Research page and an opened research (`open.ts`, laid out like a session).
- `orb/`: the orb window (entry `orb.ts`).
- `ui/`: shared helpers with no page behind them: Tauri bindings, DOM builders, icons, formatting, menus, the page cache, and the Home and Projects row pieces.

`apps/desktop/src-tauri/src` is grouped by domain. `main.rs` starts the app
and lists every Tauri command; each `mod.rs` only declares and re-exports.

- `beep/`: the server connection (`Beep`), runs, saved state, chat sync, identity.
- `bar/`: the bar window and its app-wide, live and window commands.
- `chat/`: chats, groups, approvals, files, history and reply rendering.
- `fleet/`: the mesh this Mac belongs to and the node in this process: agents, files, projects, terminals, alerts.
- `voice/`: live mode, voice notes, audio, the Option hotkey, the orb.
- `center/`: the command center window, Home, view cache, usage, what is shown.
- `palette/`: the Cmd+K search.
- `projects/`: project list, import, page, issues, PRs, runs, reviews, sync.
- `agents/`: coding agent sessions, files, diffs, artifacts, harnesses, sign-ins.
- `machines/`: the account's machines, terminals, and this Mac as a node.
- `settings/`: apps, credentials, git sign-ins, grants, memories, routines, skills, models.
- `research/`: research commands and what the team did.
- `notices/`: routing server notices, macOS notifications, sub-agent questions, the bell's history from the mesh.
- `tray/`: the tray menu and what waits on the user.
- `platform/`: log file, login `PATH`, editors, app updates.

## beep-node releases

`just node-release 0.1.1` sets the version in `crates/node/Cargo.toml`,
updates both lockfiles, commits and tags `node-v0.1.1`. Pushing the tag runs
`.forgejo/workflows/node.yml`: static Linux musl builds (x86_64, aarch64)
from the Dockerfile's `node-dist` stage, a Windows x86_64 zip, an Apple
silicon build on the Mac runner, then a Forgejo release and an upload to R2 at
https://releases.flashmind.com/beep/node/. Machines install it with:

    curl -fsSL https://releases.flashmind.com/beep/node/install.sh | sh

then join the mesh with `beep-node mesh join <ticket>` (or `-` to read the
ticket from stdin), with the ticket from the desktop's Add a machine. It starts
the node in the background if none runs. `--search <dir>` sets the folder to
look for git checkouts in. `beep-node mesh devices` lists the devices and which
are online. `beep-node setup <token>` is the older way: it links the node to a
server account and installs the service. A node binds UDP port 47120 for
iroh (`port` in `node.toml`; 0 is random), so a firewall can allow that one
port; if it is taken the node warns and binds a random one. The server
binds 47121 (`port` in its config). Rerunning the installer upgrades
in place and restarts the service. There is no Intel Mac build.
`beep-node update` does the same from the node itself: it checks
`latest.txt`, verifies the archive against `SHA256SUMS`, swaps its own binary
and restarts the service `setup` installed (`--check` only reports).
The release also pushes the `beep-node-slim` image from the same musl binaries
(target `node-image`, `deploy/node-entrypoint.sh`; README, beep-node in Docker).

`beep-node status` (`s`, `st`) asks the running node over its control socket
(`mesh.sock`, `status` answers a `LocalStatus` in compact msgpack): version and
uptime, the mesh's devices with when each was last seen, and each project's
sync. `--help` groups the commands; the groups are in `src/cli/help.rs`, and a
command's first doc paragraph is its one line there.

On Windows, `crates/node/install.ps1` does the same from PowerShell
(`irm https://releases.flashmind.com/beep/node/install.ps1 | iex`): it
installs `beep-node-<version>-x86_64-pc-windows-msvc.zip` into
`%LOCALAPPDATA%\Programs\beep-node`, and `setup` registers the scheduled task
"Beep node". The release builds that zip on the host runner with cargo-xwin
(the Dockerfile's `node-dist-windows` stage). Updates move the running exe
aside to `beep-node.exe.old`, removed by the next update.

`send_files` works like scp and needs no shared folders: any path here, any folder there (made if missing), and always directly over the mesh when the target is in it. Other file access (`files/1`, project sync, session moves) keeps to `files` in `node.toml`.

## beep-sync and git

The whole checkout syncs: tracked files, ignored files and `.git`
(`crates/node/src/files/sync/gitdir.rs` says what each path in `.git` does).
`.beepignore` and the built-in defaults still apply, except that a tracked file
stays in spite of a default. The `.stignore` of any folder at or above a project
applies too; one it cannot read refuses the folder and says why.

- Objects add up: a peer's `git gc` deletes nothing here. Refs are read from git
  and written with `git update-ref`, once all they name is here, so the reflog
  keeps a ref moved backwards. Each machine rebuilds its own index.
- Commits on both sides of a branch: the later commit keeps the branch, and the
  other stays as `refs/beep-conflicts/<ref>-<tag>`, listed as a conflict. If one
  is an ancestor of the other, the branch moves forward and the ref goes.
- Two checkouts merge whatever commits they are at, as with Syncthing: the newer
  version of a file wins and the other becomes a conflict copy.
- Locks, `FETCH_HEAD`, reflogs, hooks and `.git/worktrees` stay on their
  machine, and so does any folder whose `.git` is a file (a linked worktree or
  a submodule). Its branch still travels; it waits on a machine that has it
  checked out in a linked worktree.
- Peers talk `beep-sync/3`. Nodes on `beep-sync/1` or `/2` do not sync with it.
- Add on a project's machine row makes the folder there and joins sync: the empty
  folder takes the files and `.git` from the others (`Desk::add_to_machine`).
- Change folder on a row points the machine at another folder, empty (filled as by Add)
  or a checkout of the same repo (it merges); nothing moves and the old folder stays
  (`Desk::change_folder`).
- A member's folder that is deleted is made again and filled from the others, deleting
  nothing elsewhere; with the folder above it gone too (a disk not mounted) it waits
  (`Syncs::check_folders`). A project a node does not run is in its status with why.
- Remove on a row takes the machine out of `sync/<name>` (the record goes below two)
  and deletes its place; the node stops and its folder stays untouched
  (`Desk::remove_from_machine`). The last machine and one with an open agent session
  are refused. Only an import or Add writes a place, so it does not come back by itself.
  Ticked, the folder there is deleted once its node no longer lists the project, never a
  disk root, home, Syncthing share or a folder another project shares (`left_folder.rs`).

A session also moves on a project Syncthing keeps in step: folders come from the place records when `sync/<name>` does not name the machine, and the target trusts Syncthing, waiting up to two minutes for its folder to be complete. A project neither keeps is refused before the source stops.

## Agent skills across the mesh

Each node (the Mac app's too) lists the user's skills on `skills/1`
(`crates/node/src/agents/skills/`): every `<name>/SKILL.md` folder under
`~/.claude/skills`, `~/.agents/skills`, `~/.codex/skills`, `~/.pi/agent/skills`,
`~/.gemini/skills` and `~/.config/opencode/skills`, hashed over its files.
Beep's own (the "Installed by Beep" marker) are left out. Every two minutes,
when a machine comes or goes, and when a peer says its skills changed
(`Changed`), a node reads the others' lists.

`skills-seen` in the node folder records each name held and the copy last
synced (hash and version, the version counting edits). A listed copy carries
that version and whether it was edited since. A skill never held is copied
into `~/.claude/skills` and `~/.agents/skills`. An edit spreads: a copy still
as last synced is replaced by the newer one (staged beside it, then swapped).
Two copies edited apart, or an edit made to an older version, are a conflict:
nothing changes, the node logs a warning and the desktop shows a bell row. Two
local folders follow the same rule. Deletes never spread, and a name in
`skills-seen` that is gone here is never copied back.

## iOS app

`apps/ios` is the SwiftUI port. `just ios-run` builds, installs and launches
on a connected iPhone with Developer Mode enabled. Pass a device ID or name
to select one; `IOS_TEAM_ID` overrides the local development certificate's team.
`just ios-sim-run` runs on the iPhone 17 simulator. Debug builds take
`-send "text"` and `-open
chats|settings|memories|routines` as launch arguments
(`xcrun simctl launch booted com.flashmind.beep -send "hi"`), so it can be
tested without tapping.

## Agents that start agents

A coding agent starts another, of any harness, on any machine, with the
`start_agent` tool of the Beep MCP server (`agent_status`, `send_to_agent`,
`stop_agent`, `list_agents` follow it). The child is a normal session whose
`SessionInfo::parent` names the caller. Code: `crates/node/src/agents/family.rs`
(links, limits, cascade) and `mcp/children.rs` (the tools). A node answers
them itself, never the server. Another machine's node takes them as `tools/1`
requests (`Tool::StartAgent` and friends, which an older node reads as
`Unknown`), and a desktop refuses `start_agent` from another node.

- Depth at most 3 and at most 8 live agents per root, checked by the node
  that starts the child from the `ParentLink` the caller passes (depth,
  root, `others` counted on earlier machines). A node runs 8 sessions at
  most, so on one machine a root has 7 children.
- An agent reaches only its own children, by the same refusal for an id that
  is not its own as for one that does not exist. Only the user answers a
  child's permission questions. A child never runs above its parent's mode:
  a parent below full access starts only children of its own harness, in its
  mode.
- Stop (the turn) on a parent stops its children's turns, here and on other
  machines. A parent that ends for good closes its children. A parent that
  ends with the node does not: both resume from `reopen.json` (`OpenSession`
  keeps `parent` and the children on other machines). A child whose parent's
  node crashed stays running and shows as a top-level session.
- `wait` returns after 45 seconds at the latest with the child's state:
  Codex gives an MCP tool call 60 seconds by default (`tool_timeout_sec`),
  and Claude Code far longer, so a long job is polled with `agent_status`.
- Spend is reported per session (`tokens`, `cost_usd` in the status) and never
  added to the parent's.
- In the desktop a child is a card in the parent's log (the subagent card,
  with a `[child]` line) and nests under its parent in the session list.
  `MachineSession` has no parent; the desktop reads it from the node's
  `SessionInfo` (`fleet/parents.rs`).

To make a harness's own subagent tool hand work to `start_agent`, add a
project-level agent file. Beep does not install these. For Claude Code,
`.claude/agents/codex-worker.md`:

    ---
    name: codex-worker
    description: Hands a self-contained task to a Codex agent through Beep and reports its answer.
    tools: mcp__beep__start_agent, mcp__beep__agent_status
    ---
    Call mcp__beep__start_agent once with harness "codex", the whole task as
    the prompt and wait true. While its state is working, call
    mcp__beep__agent_status with its id. Answer with its last_message.

For Codex, `.codex/agents/claude_worker.toml` (lowercase letters, digits and
underscores in the name; `description` and `developer_instructions` are
required) says the same with `harness` set to `claude` and the tools named
`start_agent` and `agent_status`.

## Code map: beep-node

`crates/node/src`, one line per top-level module:

- `agents/`: coding agents run over ACP, their sessions and spend, sessions
  found on disk (`claude/`, `found/`), the Beep MCP server (`mcp/`) and the
  agents that start agents (`family.rs`, `mcp/children.rs`).
- `files/`: shared folders (`Folders`), project sync, Syncthing checks,
  node-to-node transfers and the git checkout search.
- `link/`: the node's key and settings (`state`), linking and the control
  stream, reconnecting, and what carries over a restart.
- `machine/`: what the machine has (`env`, `stats`), setup, the service,
  self-update and provisioning.
- `run/`: process groups, shell sessions, background tasks and terminals.
- `lib.rs` declares these; `main.rs` is the command line, with its grouped
  help and `status` output in `cli/`.
