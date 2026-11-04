# Beep

A personal assistant with a Rust server and phone and desktop clients. The
server owns model credentials, prompts, tools, conversations and memory, and
runs agents with Flashmind. Clients connect over iroh (QUIC with NAT
traversal), so there are no certificates or port forwarding.

```text
crates/proto/           Wire types, framing and the iroh client
crates/server/          Server, beep-probe/beep-models, tests
  prompts/              Assistant prompt
  migrations/           Postgres schema
crates/android-bridge/  UniFFI wrapper of the client for Kotlin
apps/android/           Kotlin/Compose app
apps/desktop/           Tauri tray app (macOS)
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

The server prints its node ID on startup. `just bake-profile` writes it to
`.dev/android-assets/testing-profile.json`, which the app builds pick up.

Each client signs in with its own iroh key. A key the server has not seen
becomes a new account, named from the key (e.g. `mega-fox-trot`). The Android
app backs its key up to Google Block Store, or to a file from Settings.

Check the endpoint with the probe:

```sh
BEEP_IDENTITY=.dev/probe-key cargo run -q -p beep-server --bin beep-probe -- NODE_ID health
```

## Android

Needs JDK 17 or 21, SDK 36, NDK `28.2.13676358` and
`rustup target add aarch64-linux-android armv7-linux-androideabi
x86_64-linux-android i686-linux-android`. Gradle runs
`scripts/build-android-native.sh` to build the Rust library and Kotlin
bindings.

```sh
just android          # bake profile, build debug APK, install, launch
cd apps/android && ./gradlew :app:testDebugUnitTest
```

Bump `versionCode` on every release so updates install over old builds.

## iOS TestFlight

The `ios` Forgejo workflow uploads every push to `main` to TestFlight. The
App Store Connect app uses bundle ID `com.flashmind.beep`. The Mac runner needs
Xcode, Rust, Homebrew and access to the sibling Rust dependency repository.

Set repository variables `BEEP_NODE_ID` (64 lowercase hex characters) and
`IOS_TEAM_ID` (Apple Developer team ID). The workflow uses these repository
secrets. The Apple API key secrets are also used by the desktop workflow:

- `APPLE_API_PRIVATE_KEY`: App Store Connect API `.p8` contents
- `APPLE_API_KEY_ID` and `APPLE_API_ISSUER_ID`: IDs for that API key
- `IOS_DISTRIBUTION_P12`: base64-encoded Apple Distribution certificate and private key
- `IOS_DISTRIBUTION_PASSWORD`: password for the `.p12` file
- `IOS_PROVISIONING_PROFILE`: base64-encoded App Store Connect provisioning profile

Xcode uses the distribution certificate and profile to sign the upload. Create
an internal TestFlight group, then a private external group after Apple processes
the first upload. Invite external testers by email. Apple must approve the first
build before they can install it.

## Wire protocol

ALPN `beep/2`. Each request opens its own bi stream of length-delimited
frames: a tag byte, then a msgpack body. The server pushes notices to clients
on `beep-push/1`. See `crates/proto/src/transport/wire.rs`.

## Tests

```sh
just test     # starts Postgres, runs the server tests
just eval     # prompt evals against the real model (costs money)
```

The integration tests open real loopback iroh connections with a
deterministic model provider.
