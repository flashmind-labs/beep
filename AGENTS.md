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

See [AGENT_REFERENCE.md](AGENT_REFERENCE.md) for feature behavior, code maps,
protocols, deployment and release details. Check the implementation when
changing those features; the reference may lag behind the code.

## UI

Update the platform mock before changing an app's UI, then match the app to it:
`mockups/android/index.html`, `mockups/ios/index.html` or
`mockups/desktop/index.html` (including the tray bar).

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
app. Release recipes commit and tag; `just release` also pushes. See the
reference and the recipes before running them.

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
protocol's bytes only with a protocol name bump. See the reference for current
ALPNs and exceptions.

## Project sync

After the initial checkout copy, Git metadata is local. Exchange commits with
Git; file sync waits when checkouts have different commits. Linked worktrees and
submodules stay on their machine. Syncthing-held folders follow Syncthing until
it pauses or releases them.

## Reports

Use Beep MCP `beep_status` and `beep_logs` to debug; `file_report` asks the user
before sending. Reports contain private logs: issues must contain only the
report reference, never logs or names. `just reports`, `just report <reference>`
and `just report-solved <reference>` manage stored reports.
