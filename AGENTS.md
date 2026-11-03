# Agent notes

## Getting the app onto the phone

The Android workflow (`.forgejo/workflows/android.yml`) builds the signed
bundle and uploads it to Play, and the phone updates from Play:

- Every push to main goes to the internal track.
- `just android-release 1.2.3` sets `versionName`, commits and tags
  `android-v1.2.3` (it needs a clean tree on main). Push it with
  `git push origin main android-v1.2.3`.
- A patch tag (`1.2.3`) goes to the `beta` track, or to the track in the
  repository variable `PLAY_BETA_TRACK`. A minor or major tag (`1.3.0`,
  `2.0.0`) goes to production. A tag that does not match `versionName` fails.
- Releases stay drafts until the repository variable `PLAY_RELEASE_STATUS` is
  `completed`.

To release Android, the desktop app and beep-node together, run
`just release`. It bumps each app's patch number from its latest tag, runs the
release recipes and pushes main with the tags. Pass versions to pick them:
`just release 0.2.0 0.4.0 0.1.1` (Android, desktop, then node). The node is
left out until its first `node-v` tag exists.

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

## Deploying the server

Push to main, then `just deploy-server`. It waits for the image CI builds for
HEAD, copies the dev config and key to the remote host and restarts the
container.

Tag a deployed server with `just server-release 2.1.0` (it tags HEAD
`server-v2.1.0`). To roll back, `just deploy-server server-v2.0.0` deploys
that tag's image. Migrations are not undone, so an older server must still
run against the newer schema.

## UI changes

Every screen of the app is mocked in the Beep screens artifact:
https://claude.ai/artifact/BwYePcE9BHvLcYSvkRwvxD

Before changing the Android UI, update the mock first so it shows the new
screen or state, then change the app to match.

## App tools

The agent's app tools come from two places:

- Composio toolkits (Gmail, Google Calendar, Outlook, Notion, Linear,
  Todoist, GitHub, Slack, ...), named by Composio slug (`GMAIL_MOVE_TO_TRASH`).
  flashmind calls Composio's v3.1 API; some slugs are missing from v3, so
  check against v3.1.
- Fastmail's remote MCP server, exposed as one flat tool per entry in
  `FASTMAIL_TOOLS` (`crates/server/src/apps/mcp.rs`) and named
  `FASTMAIL_<TOOL>`. The server caches its schemas in `mcp_servers`.

`CATALOG` in `crates/server/src/apps/mod.rs` lists the tools the agent gets
and which of them act. An acting tool asks first when the app is set to Ask
(`crates/server/src/permissions.rs`). The approval card shows the call's
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
`crates/proto/src/version.rs`, and bless the new `tests/wire/<n>` directory.

## Desktop app

`apps/desktop` is the macOS tray app (Tauri 2, its own Cargo workspace). Run
it with `just desktop-dev`, check it with `just desktop-check`, and build the
DMG with `just desktop-build` (it lands in `target/beep-macos.dmg`; the
installer window comes from `apps/desktop/installer/`). The `desktop` Forgejo
workflow builds the same DMG on the Mac runner. Its key and state live in
`~/Library/Application Support/com.flashmind.beep.desktop/`. The bar's screens
are in the same mock as the phone's.

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

## beep-node releases

`just node-release 0.1.1` sets the version in `crates/node/Cargo.toml`,
updates both lockfiles, commits and tags `node-v0.1.1`. Pushing the tag runs
`.forgejo/workflows/node.yml`: static Linux musl builds (x86_64, aarch64)
from the Dockerfile's `node-dist` stage, an Apple silicon build on the Mac
runner, then a Forgejo release and an upload to R2 at
https://releases.flashmind.com/beep/node/. Machines install it with:

    curl -fsSL https://releases.flashmind.com/beep/node/install.sh | sh

then link with `beep-node setup <token>`. Rerunning the installer upgrades
in place and restarts the service. There is no Intel Mac build.
`beep-node update` does the same from the node itself: it checks
`latest.txt`, verifies the archive against `SHA256SUMS`, swaps its own binary
and restarts the service `setup` installed (`--check` only reports).

On Windows, `crates/node/install.ps1` does the same from PowerShell
(`irm https://releases.flashmind.com/beep/node/install.ps1 | iex`): it
installs `beep-node-<version>-x86_64-pc-windows-msvc.zip` into
`%LOCALAPPDATA%\Programs\beep-node`, and `setup` registers the scheduled task
"Beep node". The release workflow has no Windows job yet.

## iOS app

`apps/ios` is the SwiftUI port. `just ios-run` builds, installs and launches
on a connected iPhone with Developer Mode enabled. Pass a device ID or name
to select one; `IOS_TEAM_ID` overrides the local development certificate's team.
`just ios-sim-run` runs on the iPhone 17 simulator. Debug builds take
`-send "text"` and `-open
chats|settings|memories|routines` as launch arguments
(`xcrun simctl launch booted com.flashmind.beep -send "hi"`), so it can be
tested without tapping.
