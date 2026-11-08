# Beep

A personal assistant with a Rust server and phone and desktop clients. The
server owns model credentials, prompts, tools, conversations and memory, and
runs agents with Flashmind. Apps reach it over HTTP. Desktops and
`beep-node`s form a mesh over iroh with no server; the server never reaches
a node.

```text
crates/proto/           Wire types, framing and the iroh client
crates/server/          Server, beep-probe/beep-models, tests
  prompts/              Assistant prompt
  migrations/           Postgres schema
apps/desktop/           Tauri app: macOS and Windows tray app, and the
                        iOS and Android phone app
deploy/                 Container entrypoint and AppArmor profile
scripts/                Build helpers
```

## Server

Requires Rust 1.93+, the sibling `../flashmind` repository, and Postgres with
pgvector (`just test-db` starts one in Docker).

```sh
cp config.example.toml config.toml   # set model and provider_key
cargo run -p beep-server -- config.toml
```

Each client signs in with its own iroh key. A key the server has not seen
becomes a new account, named from the key (e.g. `mega-fox-trot`).

Check the endpoint with the probe:

```sh
BEEP_IDENTITY=.dev/probe-key cargo run -q -p beep-server --bin beep-probe -- http://127.0.0.1:47122 health
```

## Self-hosting

`deploy/compose.yaml` runs beep-server and Postgres with Docker Compose.

1. `cp config.example.toml deploy/config.toml` and edit it: set `model` and
   `provider_key`, remove `database_url` (compose passes `DATABASE_URL`), and
   set `secret_key = "/data/iroh-key"` and `http_listen = "0.0.0.0:47122"`.
2. `POSTGRES_PASSWORD=<something long> docker compose -f deploy/compose.yaml up -d`

The image is built from this checkout; set `BEEP_SERVER_IMAGE` to use one
you built or pulled elsewhere. The iroh key and logs live in the `data`
volume.

Ports:

- HTTP for the apps on `http_listen`, default `127.0.0.1:47122`. Compose
  publishes it on the host's loopback only. How it reaches the internet (a
  tunnel, or a reverse proxy with TLS) is your choice.

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

## Wire protocol

ALPN `beep/2`. Each request opens its own bi stream of length-delimited
frames: a tag byte, then a msgpack body. The server pushes notices to clients
on `beep-push/1`. See `crates/proto/src/transport/wire.rs`.

## Tests

```sh
just test     # starts Postgres, runs the server tests
just eval     # prompt evals against the real model (costs money)
```

The integration tests talk to the server over loopback with a
deterministic model provider.

### Windows desktop

Windows x64 needs the WebView2 runtime. With Rust's MSVC toolchain, Visual
Studio C++ Build Tools and Node installed, run `scripts/build-desktop-windows.ps1`
in PowerShell to build the current-user NSIS installer.

On Linux with cargo-xwin and its Clang/LLVM prerequisites, run
`scripts/build-desktop-windows.sh` to build a portable executable.
Ctrl+Alt+Space opens the bar; Control replaces Command in app shortcuts.
Desktop control and live voice are unavailable on Windows.
