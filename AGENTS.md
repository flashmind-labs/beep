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
`mockups/desktop/index.html` (including the tray bar), and for the phone
layout of the same app `mockups/ios/index.html` or `mockups/android/index.html`.

## Code structure and readability

- Use established libraries for standard parsing and shared behavior instead
  of writing custom parsers or utility implementations.
- Keep related types, constants and functions together in existing modules.
  Do not create a file for a single declaration or an unnecessary wrapper.
- Separate logical stages in functions with blank lines, including before
  control-flow blocks and after them before subsequent work or a tail return.
  Keep cohesive setup and short related updates together.

## Rust layout

Group code by responsibility in small modules, following `crates/node` and
`crates/server`. Keep `mod.rs` focused on declarations, re-exports and a thin
public API. Preserve public paths and wire formats when moving code.

Keep unit tests together in the crate's `src/tests/`, grouped by module. Declare
them at the end of the module with `#[cfg(test)]` and `#[path]` so they can
exercise private code. Public API tests belong in the crate's `tests/`.

## TypeScript and frontend development

- Write application code in TypeScript, not plain JavaScript. Use its type
  system to enforce domain rules and catch invalid states before runtime.
- Use named domain types and discriminated unions for identifiers, navigation,
  requests and state. Prefer exhaustive handling over generic strings,
  loosely typed objects and collections of optional fields.
- Avoid `undefined` in application models and APIs. Model absence explicitly
  with `null` or a named union variant; make required values required. Handle
  missing values from browser and library APIs at the boundary.
- Avoid `any`, non-null assertions and casts that bypass checking. Narrow and
  validate external data before using it.
- Prefer established frameworks and typed libraries for UI, navigation,
  keyboard shortcuts and other shared behavior over handwritten DOM wiring
  and scattered event handlers. Choose tools that fit the application.
- Organize modules around a clear responsibility. Put the main flow before
  supporting details where practical, so code reads from top to bottom.
  Keep state ownership explicit and related logic together.

## Rust types and conversions

Use domain types for identifiers, names with domain meaning, and paths. Reserve
`String` for actual text. Use enums for fixed protocol and state sets, derive
`PartialEq` and `Eq`, and match variants directly. Convert strings at external
boundaries; preserve unknown wire values with a fallback variant when needed.
Prefer `From`, `TryFrom`, `FromStr`, `AsRef` and
`Display` over custom conversion methods such as `from_acp` or `as_str`. Use
Strum derives for enum conversions when they preserve the exact accepted values,
labels and errors. Prefer `EnumString`, `AsRefStr`, `IntoStaticStr` and `Display`
over repeated manual matches; use `#[strum(serialize = "...")]` where names differ.
Keep validation and error handling in the conversion traits, and preserve serde
and wire representations independently. Prefer explicit
`Into::<Target>::into(value)` over inferring the target from `value.into()`.

## Tests and development

- Server tests need Postgres: use `just test`; plain `cargo test -p beep-server`
  needs `BEEP_TEST_DATABASE_URL`.
- The desktop is a separate Cargo workspace. Use `just desktop-dev`,
  `just desktop-check` and `just desktop-build`.
- The phone app is `apps/desktop` built for iOS and Android. Use `just ios-run`
  for a connected iPhone, `just ios-sim-run` for the simulator and
  `just android-run` for Android. On a phone it is a mesh observer
  (`cfg(mobile)`): no node, no agents, no terminals hosted.
- For app tools, check actual arguments with `just tool-schema` before coding.
  When adding an acting tool that takes an ID, add its read tool to `LOOKUPS`
  in `crates/server/src/apps/permissions/`.

## Changelog

Every agent must add user-visible changes under `[Unreleased]` before finishing
work: desktop in `changelogs/desktop.md`, other apps in `changelogs/node.md`,
`changelogs/android.md`, `changelogs/ios.md`, or `changelogs/server.md`.
Record shared changes in each affected app. Use `Added`, `Changed`, `Fixed`,
`Security`, then `Performance`; write from the user's view and name the platform.
Skip internal churn. Never invent past release notes or edit published entries.

Release preparation moves each app's Unreleased notes into a dated version
entry. All release paths must freeze the matching changelog. Desktop and node
release pages and desktop updater feeds use those notes. Keep an empty
Unreleased section for the next changes. Never publish without release notes.

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
- `apps/desktop/src-tauri/gen`: the iOS and Android projects, generated by
  `scripts/tauri-init.sh` and never committed.

## Release details

Desktop and node tags trigger signed builds and release uploads. The phone
app is the desktop Tauri app: `android-v*` and `ios-v*` tags build it and
upload to Play and TestFlight, and each phone version lives in
`tauri.android.conf.json` or `tauri.ios.conf.json`. Read the relevant
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
