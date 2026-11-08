# Agent notes

## Project

Desktops, phones and `beep-node`s communicate directly over a signed mesh.
Phones pair over iroh with a desktop's one-use ticket, valid for five minutes.
Desktop and node membership persists; a paired phone reaches nodes without a
running desktop. Phones use the desktop controller role for shared projects,
groups, pins, credentials and settings. Nodes do not connect to `beep-server`.

Assistant API keys belong only on the server. Agent and forge credentials may
be shared with nodes. Assistant, server and live voice features are disabled
on desktop and phones by `SERVER_ENABLED` and the live voice guards.

Check the implementation when changing a feature; these notes may lag behind it.

## Feature requests

Every feature the user asks for gets a GitHub issue in `flashmind-labs/beep`
(`gh issue create`) before work starts. Link it from the commit or close it
when the feature ships.

## UI

Update the platform mock before changing an app's UI, then match the app to it:
`mockups/android/index.html`, `mockups/ios/index.html` or
`mockups/desktop/index.html` (including the tray bar).

## Rust types and conversions

Use domain types for identifiers, names with domain meaning, and paths. Reserve
`String` for actual text. Prefer `From`, `TryFrom`, `FromStr`, `AsRef` and
`Display` over custom conversion methods such as `from_acp` or `as_str`. Use
Strum derives when they express the conversion correctly, including `EnumString`,
`AsRefStr`, `IntoStaticStr` and `Display`. Keep validation and error handling in
the conversion traits. Prefer explicit `Into::<Target>::into(value)` when using `Into`,
rather than inferring the target from `value.into()`.

## Tests and development

- Server tests need Postgres: use `just test`; plain `cargo test -p beep-server`
  needs `BEEP_TEST_DATABASE_URL`.
- The desktop is a separate Cargo workspace. Use `just desktop-dev`,
  `just desktop-check` and `just desktop-build`.
- Use `just ios-run` for a connected iPhone and `just ios-sim-run` for the
  simulator.
- Do not hand-install APKs on the phone: local signing differs from Play. Use
  `./gradlew assembleDebug` on an emulator.
- For app tools, check actual arguments with `just tool-schema` before coding.
  When adding an acting tool that takes an ID, add its read tool to `LOOKUPS`
  in `crates/server/src/apps/permissions/`.

## Releases and deployment

Apps remain on 0.x until production, with minor or patch bumps only. Do not run
a release recipe, tag or upload a build without cutting a new version for that
app. Release recipes commit and tag; `just release` also pushes. Read the recipes and workflows before running them.

Production is deployed from the private ops repo. Ask before deploying the
server, and before stopping or recreating it. The on-prem bundle is retired.

## Wire compatibility

Released phones must keep working. App messages use named msgpack:

- Add fields with `#[serde(default)]`; never rename or retype fields, or remove
  fields a released app sends or reads.
- Add reply enum or `AppEvent` variants only behind a new request. Never reuse
  request numbers or change request path names.
- Keep the fixtures in `crates/proto/tests/wire/named/` unchanged. Create a
  fixture for each new request with `BEEP_WIRE_BLESS=1 cargo test -p beep-proto compat`.

Node and mesh messages use positional msgpack. Append fields with
`#[serde(default)]`; never insert, reorder, retype or remove fields, or use
`skip_serializing_if` or `to_vec_named`. Read using
`beep_proto::lenient::from_slice`.

Signed mesh types (`Device`, `OpBody`, `Signed`) cannot gain fields in place:
change `SIGN_DOMAIN`, mesh ALPNs and `MESH_DIR` together. Terminal host
`Request`, `Reply` and `Hello`, and transfer `Manifest`, cannot gain fields
while released readers using plain `rmp_serde` remain reachable. Change a
protocol's bytes only with a protocol name bump. The current mesh ALPNs are `beep-peer/2` and `beep-join/2`; sync uses
`beep-sync/7`.

## Reports

Use Beep MCP `beep_status` and `beep_logs` to debug; `file_report` asks the user
before sending. Reports contain private logs: issues must contain only the
report reference, never logs or names. `just reports`, `just report <reference>`
and `just report-solved <reference>` manage stored reports.


## Code map

- `crates/mesh`: signed membership, tickets and replicated state.
- `crates/fleet`: desktop-side projects, credentials, settings and mesh operations.
- `crates/node`: agents and their MCP tools, files and sync, mesh, machines and processes.
- `crates/proto`: app messages and wire compatibility fixtures.
- `crates/server`: server API, agents, app tools, storage, memory and voice.
- `apps/desktop/src`: UI grouped by feature; `*-state.ts` contains tested logic.
  `apps/desktop/src-tauri/src` contains the desktop commands and mesh integration.
- `apps/android` and `apps/ios`: phone apps; `crates/android-bridge` serves both.

## Release details

Android and iOS release tags trigger Play and TestFlight builds. Desktop and
node tags trigger signed builds and release uploads. Read the relevant
`.forgejo/workflows/` file and `justfile` before releasing. Android releases
stay off production while apps are on 0.x.

## Mesh operations

A desktop creates one-use pairing tickets. Nodes join the mesh and never mint
tickets. A temporary machine is removed after 24 hours offline; other machines
are not removed automatically.

A new Git project copies the committed checkout and Git metadata once. After
that, repositories exchange commits through Git. Uncommitted edits stay local
until an explicit handoff to a clean checkout at the same commit. Non-Git
folders keep file sync. Linked worktrees and submodules stay on their machine.
Syncthing project exclusions apply on every imported machine, including when
offline machines reconnect; the shared folder is never paused.
