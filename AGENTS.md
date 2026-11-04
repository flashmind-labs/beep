# Agent notes

## Getting the app onto the phone

The Android workflow (`.forgejo/workflows/android.yml`) builds the signed
bundle and uploads it to Play, and the phone updates from Play:

- Only an `android-v` tag builds and uploads; a manual run of the workflow
  goes to the internal track.
- `just android-release 1.2.3` sets `versionName`, commits and tags
  `android-v1.2.3` (it needs a clean tree on main). Push it with
  `git push origin main android-v1.2.3`.
- A patch tag (`1.2.3`) goes to the `beta` track, or to the track in the
  repository variable `PLAY_BETA_TRACK`. A minor or major tag (`1.3.0`,
  `2.0.0`) goes to production. A tag that does not match `versionName` fails.
- Releases stay drafts until the repository variable `PLAY_RELEASE_STATUS` is
  `completed`.

iOS works the same way: `just ios-release 1.2.3` sets `MARKETING_VERSION` in
`apps/ios/project.yml`, commits and tags `ios-v1.2.3`. Pushing the tag uploads
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

One `beep-server` runs the agent and owns the database. Every user gets a
`beep-node` started with `--workspace /home/user` (`BEEP_NODE_WORKSPACE`):
`exec` and `process` run there by default, and uploads, downloads and app
attachments are saved there. Other machines link as ordinary nodes. The
server runs no user code. Per-user nodes are set up by hand for now.

The server is the production server on Neon, the target of
`just deploy-server`. Desktop and iOS builds pass its node id in
`BEEP_NODE_ID`. Ask before deploying to it, and never stop or recreate it
without asking. The on-prem bundle (`deploy/onprem/`) is retired.

### Code map

`crates/server/src`, one line per top-level module:

- `account`: `User`, single-user instance mode, guest keys, credentials.
- `agent`: around a turn: models, metering, compaction, gates, titles, sub-agents.
- `api`: request handlers, `routes`, the op `router`, the iroh `transport`.
- `app`: the `App` hub: admission, `submit`, the turn itself in `run/turn.rs`.
- `apps`: app catalog, Composio, MCP, `permissions`, `grants`, mail and Slack `feeds`.
- `config`: the TOML config.
- `memory`: memories, skills, profile, starters, watchers, the group leak check.
- `nodes`: node connections (`serve`), linking, per-node requests, the workspace.
- `projects`: project agent: tasks, workers, reviews, imports, `repo_watch`, `git_items`.
- `push`: notice delivery, FCM, APNs.
- `research`: research papers: planning, team tools, PDF.
- `routines`: what wakes the agent: `cron`, `places`, `events`, `digest`, `wake`.
- `store`: Postgres queries, one file per kind of row, `migrations`.
- `tools`: the agent's tools, plus web `search` and `fetch`.
- `voice`: voice notes, live transcription (`stt`), filler lines.

## Deploying the server

Push to main, then `just deploy-server`. It waits for the image CI builds for
HEAD, copies the dev config and key to the remote host and restarts the
container.

Tag a deployed server with `just server-release 2.1.0` (it tags HEAD
`server-v2.1.0`). To roll back, `just deploy-server server-v2.0.0` deploys
that tag's image. Migrations are not undone, so an older server must still
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

## Server tests

`just test` starts the local Postgres and runs the server tests. A plain
`cargo test -p beep-server` fails without `BEEP_TEST_DATABASE_URL`.

## Wire compatibility

Phones update from Play on their own schedule, so the server must keep
answering every released app. rmp_serde encodes structs as arrays and refuses
an element it does not expect, so:

- Requests may only gain `#[serde(default)]` fields, always added at the end.
  Never insert, reorder, retype or remove a field: fields are read
  by position, so everything after the change would shift. A server skips
  trailing fields it does not know, so a newer app still works against it
  (servers from before 196b52f refuse them: deploy the server first).
- Replies, pushed `Notice`s and streamed enums never change shape. A new field
  or variant goes behind a new request, or behind a `Feature` the server checks
  with `cx.supports(...)` (mark a `wire_enum!` variant `since FEATURE`).
- Request numbers are never reused or removed.
- Fixtures in `crates/proto/tests/wire/` are append-only. A new request gets
  them with `BEEP_WIRE_BLESS=1 cargo test -p beep-proto compat`; never edit or
  delete one to make the test pass.

Every connection opens with `Hello`, which carries the app's `WIRE`; apps from
before it are wire 1. To ship a new shape, add a `Feature`, raise `WIRE` in
`crates/proto/src/transport/version.rs`, and bless the new `tests/wire/<n>` directory.

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
- `center/`: the command center window (entry `center.ts`): sidebar, Home.
- `project/`: the Projects page and one project's page.
- `addproject/`: the Add a project and Import a project sheets.
- `review/`: branches, reviews and the diff viewer.
- `agents/`: coding agents: start form, picker, a session's log, the list.
- `agentpane/`: a session's side pane: media, files, changes, artifacts.
- `chat/`: chats, groups, the composer and reply cards.
- `lists/`: Memories, Routines and Skills.
- `machines/`: the Machines list, Home's machine table, Add a machine.
- `terminals/`: terminals kept running on the machines.
- `settings/`: the Settings pages and sheet.
- `research/`: the research window (entry `research.ts`) and its lists.
- `orb/`: the orb window (entry `orb.ts`).
- `ui/`: shared helpers: Tauri bindings, DOM builders, formatting, menus.

`apps/desktop/src-tauri/src` is grouped by domain. `main.rs` starts the app
and lists every Tauri command; each `mod.rs` only declares and re-exports.

- `beep/`: the server connection (`Beep`), runs, saved state, chat sync, identity.
- `bar/`: the bar window and its app-wide, live and window commands.
- `chat/`: chats, groups, approvals, files, history and reply rendering.
- `voice/`: live mode, voice notes, audio, the Option hotkey, the orb.
- `center/`: the command center window, Home, view cache, usage, what is shown.
- `palette/`: the Cmd+K search.
- `projects/`: project list, import, page, issues, PRs, runs, reviews, sync.
- `agents/`: coding agent sessions, files, diffs, artifacts, harnesses, sign-ins.
- `machines/`: the account's machines, terminals, and this Mac as a node.
- `settings/`: apps, credentials, git sign-ins, grants, memories, routines, skills, models.
- `research/`: research windows and their commands.
- `notices/`: routing server notices, macOS notifications, sub-agent questions.
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

then link with `beep-node setup <token>`. A node binds UDP port 47120 for
iroh (`port` in `node.toml`; 0 is random), so a firewall can allow that one
port; if it is taken the node warns and binds a random one. The server
binds 47121 (`port` in its config). Rerunning the installer upgrades
in place and restarts the service. There is no Intel Mac build.
`beep-node update` does the same from the node itself: it checks
`latest.txt`, verifies the archive against `SHA256SUMS`, swaps its own binary
and restarts the service `setup` installed (`--check` only reports).

On Windows, `crates/node/install.ps1` does the same from PowerShell
(`irm https://releases.flashmind.com/beep/node/install.ps1 | iex`): it
installs `beep-node-<version>-x86_64-pc-windows-msvc.zip` into
`%LOCALAPPDATA%\Programs\beep-node`, and `setup` registers the scheduled task
"Beep node". The release builds that zip on the host runner with cargo-xwin
(the Dockerfile's `node-dist-windows` stage). Updates move the running exe
aside to `beep-node.exe.old`, removed by the next update.

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
- Two checkouts never merged before sync only once they are at one commit;
  until then both show why, and neither changes.
- Locks, `FETCH_HEAD`, reflogs, hooks and `.git/worktrees` stay on their
  machine, and so does any folder whose `.git` is a file (a linked worktree or
  a submodule). Its branch still travels; it waits on a machine that has it
  checked out in a linked worktree.
- Peers talk `beep-sync/2`. Nodes on `beep-sync/1` do not sync with it.

## iOS app

`apps/ios` is the SwiftUI port. `just ios-run` builds, installs and launches
on a connected iPhone with Developer Mode enabled. Pass a device ID or name
to select one; `IOS_TEAM_ID` overrides the local development certificate's team.
`just ios-sim-run` runs on the iPhone 17 simulator. Debug builds take
`-send "text"` and `-open
chats|settings|memories|routines` as launch arguments
(`xcrun simctl launch booted com.flashmind.beep -send "hi"`), so it can be
tested without tapping.

## Code map: beep-node

`crates/node/src`, one line per top-level module:

- `agents/`: coding agents run over ACP, their sessions and spend, sessions
  found on disk (`claude/`, `found/`) and the Beep MCP server (`mcp/`).
- `files/`: shared folders (`Folders`), project sync, Syncthing checks,
  node-to-node transfers and the git checkout search.
- `link/`: the node's key and settings (`state`), linking and the control
  stream, reconnecting, and what carries over a restart.
- `machine/`: what the machine has (`env`, `stats`), setup, the service,
  self-update, provisioning, `ssh` and repository webhooks.
- `run/`: process groups, shell sessions, background tasks and terminals.
- `lib.rs` declares these; `main.rs` is the command line.
