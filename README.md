# Beep

Desktops, phones and `beep-node`s form a signed mesh over iroh. The only
server is `beep-server`, which receives the apps' problem reports.

```text
crates/proto/           Node and mesh wire types and framing
crates/mesh/            Signed membership, tickets and replicated state
crates/fleet/           Desktop-side projects, credentials and settings
crates/node/            beep-node: agents, files, sync and processes
crates/report/          Problem reports
crates/server/          beep-server: receives problem reports
apps/desktop/           Tauri app: macOS and Windows tray app, and the
                        iOS and Android phone app
deploy/                 Container entrypoints and compose file
scripts/                Build helpers
```

## Report server

```sh
cp config.example.toml config.toml
cargo run -p beep-server -- config.toml
```

Reports go to `[reports.r2]`, or to `[reports].folder` for development.
`deploy/compose.yaml` runs it with Docker Compose; it listens on loopback
port 47122 and needs a proxy or tunnel with TLS in front.
`just reports`, `just report <id>` and `just report-solved <id>` manage stored
reports.

## beep-node in Docker

Each node release publishes a small beep-node image (amd64 and arm64): the
static binary, git, curl, a shell and CA certificates. It runs as uid 1000 and keeps
its key and mesh in `/data`. To join a mesh, pass a ticket from the desktop's
Add a machine:

```sh
docker run -d --restart unless-stopped --hostname build-box \
  -e BEEP_TICKET=... -v beep-node:/data registry.example/beep-node
```

The ticket is used once, on the first start; later starts reuse `/data`. The
host name is the machine's name in the mesh. `BEEP_NODE_WORKSPACE` shares a
folder with the other devices of the mesh. Allow UDP 47120 through the firewall, or add
`-p 47120:47120/udp`. Upgrade by pulling a newer tag; `beep-node update` does
not work in the container. No coding agents are installed: derive an image
`FROM` this one and add the harnesses you use.

## Phones

The phone app is the desktop app built with Tauri for iOS and Android. On a
phone it pairs with a desktop's phone code and joins the mesh as an observer:
it runs no node and reaches machines over the mesh.

```sh
just ios-sim-run      # iPhone 17 simulator
just ios-run          # connected iPhone, signed with your development team
just android-run      # connected device or emulator
```

iOS builds need Xcode and `rustup target add aarch64-apple-ios
aarch64-apple-ios-sim`. Android builds need the SDK and NDK with
`ANDROID_HOME` and `NDK_HOME` set, and `rustup target add
aarch64-linux-android`.

### Releases

`just android-release <version>` and `just ios-release <version>` set the
phone version in `apps/desktop/src-tauri/tauri.android.conf.json` or
`tauri.ios.conf.json`, commit and tag `android-v*` or `ios-v*`. The tag makes
CI build the Tauri app and upload it to Play's beta track or to TestFlight.
`just release` tags the desktop, node, Android and iOS together.

## Tests

```sh
cargo test -p beep-server
```

### Windows desktop

Windows x64 needs the WebView2 runtime. With Rust's MSVC toolchain, Visual
Studio C++ Build Tools and Node installed, run `scripts/build-desktop-windows.ps1`
in PowerShell to build the current-user NSIS installer.

On Linux with cargo-xwin and its Clang/LLVM prerequisites, run
`scripts/build-desktop-windows.sh` to build a portable executable.
Ctrl+Alt+Space opens the bar; Control replaces Command in app shortcuts.
Desktop control and live voice are unavailable on Windows.
