# Beep agent instructions

Check the implementation before changing behavior described here.

## Workflow

- Work in a branch and linked worktree under `.worktrees/` for changes.
  Keep private specs under `docs/` out of Git, including when staging.
- Before starting a feature, create an issue in `flashmind-labs/beep` with
  `gh issue create`. Link it from the commit or close it when it ships.
  Close as completed only when main implements the whole issue and a commit
  or `file:line` shows it. Leave partial work open.
- Use one or more area labels: `area: desktop`, `area: phone`, `area: node`,
  `area: mesh`, `area: sync`, `area: server`, `area: agents` (coding agent
  sessions, ACP and MCP), `area: release` (builds, signing and CI),
  `area: assistant` (legacy server assistant), `command-center` or `plugin`.
  Use exactly one type: `bug`, `enhancement`, `performance`, `refactor`,
  `security`, `discussion` or `documentation`. Add `priority: low` when it fits.
- Have a small-model subagent draft issue titles and bodies from your notes
  and write user-facing changelog entries. Use `claude-haiku-5-5` under Claude
  ACP or `gpt-6-1-terra` under Codex ACP.

## Build and verify

| Work | Command |
| --- | --- |
| Run a separate macOS dev app | `just run apart` |
| Run with the installed app's data and mesh | Quit Beep first, then `just run` |
| Check the web frontend | `npm --prefix apps/desktop ci`, then `npm --prefix apps/desktop run check` |
| Run browser tests | `npm --prefix apps/desktop run test:browser` |
| Format, lint and test the desktop app | `just desktop-check` |
| Build the macOS DMG | `just desktop-build` |
| Run on a phone or simulator | `just ios-run`, `just ios-sim-run` or `just android-run` |
| Check pending release notes | `python3 scripts/changelog.py check` (Python 3.10+) |

Cargo's default members exclude `beep-desktop`. Use `-p beep-desktop` for
desktop Rust checks, and scope other Rust checks to the crates you touch.
Keep shared dependency versions in the root `[workspace.dependencies]`.
Generate the iOS and Android projects with `scripts/tauri-init.sh`. Never
commit `apps/desktop/src-tauri/gen/`.

For Beep failures, inspect `beep_status` and `beep_logs`. The user approves
submission through `beep_report`. Keep logs private and put only the report
reference in the issue. Use `just reports`, `just report <reference>` and
`just report-solved <reference>` to list, download and delete solved reports.

## Code map

| Path | Owns |
| --- | --- |
| `crates/mesh` | Signed membership, pairing tickets and replicated state |
| `crates/fleet` | Controller-side projects, credentials, settings and mesh operations |
| `crates/node` | Agents, MCP tools (`tools`), files, sync, machines and processes, plus `upkeep`, `occupancy` and `logs` |
| `crates/projects` | Git and project operations shared by node and desktop |
| `crates/proto` | Wire types, framing and the lenient MessagePack reader |
| `crates/plugins` | Plugin runtime and host API |
| `crates/report`, `crates/server` | Report collection and redaction, and the report receiver |
| `apps/desktop/src` | React UI in layers: `shell` (windows and routers), `screens`, `parts`, `stores` (tested logic in `*-state.ts`), `ui` |
| `apps/desktop/src-tauri/src` | Tauri commands and mesh integration |

## Mesh and sync

- Desktops, phones and nodes communicate over a signed iroh mesh. Desktops
  mint one-use tickets. A phone ticket expires after five minutes. Once
  paired, a phone can reach nodes while the desktop is offline.
- The phone builds from `apps/desktop` as an observer (`cfg(mobile)`). It has
  controller permissions for shared projects, groups, pins, credentials and
  settings. It hosts no node, agents or terminals. Agent and forge
  credentials may be shared with nodes.
- The app's Assistant runs on a compatible node, including a desktop's local
  node. `beep-server` only receives problem reports. Nodes never connect to it.
- Desktop and node membership persists. A desktop removes temporary nodes
  after 24 hours offline. Other devices stay paired until removed.
- The first Git sync copies the committed checkout and Git metadata.
  Subsequent sync exchanges commits. Uncommitted edits stay local until an
  explicit handoff to a clean checkout at the same commit. Linked worktrees
  and submodules stay on their machine. Non-Git folders sync as files.
- Apply Syncthing project exclusions on every imported machine, including
  offline machines when they reconnect. Keep the shared folder running.

### Wire compatibility

- Released phones, desktops and nodes must interoperate. Node and mesh
  messages use positional MessagePack. Append fields with `#[serde(default)]`.
  Never insert, reorder, retype or remove fields, or use
  `skip_serializing_if` or `to_vec_named`. Read with
  `beep_proto::lenient::from_slice`.
- For signed mesh types (`Device`, `OpBody`, `Signed`), a field addition
  requires a coordinated bump of `SIGN_DOMAIN`, the mesh ALPNs and `MESH_DIR`.
- Terminal host `Request`, `Reply` and `Hello`, and transfer `Manifest`,
  cannot gain fields while released readers using plain `rmp_serde` are
  reachable. Bump the protocol name to change its bytes.
- Mesh ALPNs are defined in `crates/mesh/src/wire.rs`. The sync ALPN is
  defined in `crates/node/src/files/sync/peer.rs`.

## Code conventions

- Reuse existing components, helpers, styles and libraries. Share logic
  when two callers need it. Remove code the change makes unused.
- Keep related declarations together in existing modules. Avoid forwarding
  wrappers and single-declaration files. Put the main flow first and use
  blank lines between stages, around control flow and before tail returns.
- Push values from their owner; never poll state Beep owns. Every timer and
  sleep outside tests is listed in `.timers` with its kind (`retry`,
  `debounce`, `deadline`, `outside`, `ui`, `pace`, `compat` or `poll`) and a
  reason. `compat` asks an older peer that cannot push and names the version
  that ends it.
  Add a line for a new timer. A new `poll` needs the user's approval.
- Dependencies point down: a module never imports, calls or orders what uses
  it. Upward values are pushes the upper side follows. Run `just deps` before
  merging: it fails on a new import cycle (areas and files) or a new poll.

### TypeScript and React

- Use React components with keyed lists, event props and a single state owner
  in hooks or a store. Replace manual DOM wiring when touching it. Simple
  stateless, event-free formatting can stay outside React.
- Use named types and exhaustive discriminated unions for identifiers,
  navigation, requests and state. Make required fields required. Use `null`
  or a named variant for missing values and handle `undefined` at library
  and browser boundaries.
- Validate external data. Never use `any`, non-null assertions or unchecked
  casts. Use typed libraries for navigation, popovers and shortcuts.
  `react-aria-components` is already available.

### Rust

- Group small modules by responsibility. Keep `mod.rs` to declarations,
  re-exports and a thin public API. Preserve public paths and wire formats.
- Put unit tests in the crate's `src/tests/`, grouped by module and declared at
  the end of the tested module with `#[cfg(test)]` and `#[path]`. Put public API
  tests under the crate's `tests/`.
- Use domain types for IDs, names and paths. Use `String` for text. Derive
  `PartialEq` and `Eq` for fixed sets and match the enums directly. Convert
  strings at boundaries and keep a fallback for unknown wire values as needed.
- Use `From`, `TryFrom`, `FromStr`, `AsRef` and `Display` for conversions.
  Validate in fallible conversions and keep serde independent. Use Strum
  when it preserves exact values, labels and errors. Spell out
  `Into::<Target>::into(value)`.

## UI

Change the real app directly. Update committed text snapshots in
`apps/desktop/tests/browser/` and add a test for a screen that has none.
Keep baseline PNGs local. Verify in the real app too, where WebKit layout
can differ from the browser baseline.

- Reuse `apps/desktop/src/ui/kit.tsx` for controls, rows, spinners and empty
  states. Use its `Button` and `Spinner` in feature code. Put controls shared
  by two places there. Use `icons.tsx` and `format.ts` from the same folder.
- In `apps/desktop/dist/`, put shared structure in `style.css`, `center.css`
  or `sheets.css`. Scope desktop rules in `native.css` under
  `body:not(.mobile)` and put phone rules in `mobile.css`. Give shared markup
  an unscoped default and hide desktop-only elements by default. Extend
  existing rules and group selectors with the same treatment.
- Use skin tokens from `skins.css`. Set control height with `--ctl-h` and
  fills with `--ctl-fill` and `--ctl-fill-hi`. Use `--mono` at 11 to 12px for
  chrome and small capitals for section labels. Use `--body` for titles and
  reading text. Keep corners nearly square and reserve the accent for the
  primary action, current item or running state.
- Give cards a faint fill and a lit top edge (`var(--lit)`). Separate rows
  and cards with space. Keep rows to one line where possible, show details
  below in mono and reveal secondary actions on hover or focus.
- Use glass on floating layers. Put blur on a `::before` layer so it
  leaves fixed descendants positioned correctly.
- Use `apps/desktop/src/ui/painted-art.tsx` for paintings, including `DailyArt`
  and `PaintedEmpty` for start and empty screens. Place paintings in empty
  screens, above content or beside it. Keep subjects clear of headlines and
  text clear of strokes. Keep sidebar and top haze still and clear of cards.
  Other scenes paint once per launch, move while visible and stay finished
  and still with Reduce Motion.
- Limit motion to short navigation transitions, spinners and paintings.
  Pause spinners in hidden windows. For empty states, offer one next action.
  Omit empty sections that have no action.

## Changelog and releases

- For every user-visible change, add one file per affected app under
  `changelogs/unreleased/<app>/` (`desktop`, `node`, `android`, `ios` or
  `server`). Name it after the issue or branch. Use `### Added`, `### Changed`,
  `### Fixed`, `### Security` or `### Performance` with bullets from the
  user's point of view. Name the platform. Leave other entries and published
  `changelogs/<app>.md` files alone. Skip internal changes.
- Stay on 0.x until production. Cut a minor or patch version for each app
  before tagging or uploading a build. Read `justfile`, `scripts/release.py`
  and the relevant workflow in `.github/workflows/` before releasing.
  `just patch`, `just minor` and `just release` commit, tag and push.
- Freeze the pending notes before publishing. For a manual freeze, use
  `python3 scripts/changelog.py freeze <version> --path changelogs/<app>.md`.
  Release pages and the desktop updater feed use those notes.
- Desktop and node tags trigger signed builds and uploads. `android-v*` and
  `ios-v*` upload to Play and TestFlight. Phone versions live in
  `apps/desktop/src-tauri/tauri.android.conf.json` and
  `apps/desktop/src-tauri/tauri.ios.conf.json`. Keep 0.x Android releases
  off production.
- Deploy production from the private ops repo. Ask before deploying,
  stopping or recreating the server. The on-prem bundle is retired.
