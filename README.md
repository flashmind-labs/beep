# Beep

An Android assistant backed by a Rust server using Flashmind.
The server owns model credentials, prompts, tools, conversations, and memory.
The app connects over iroh (P2P QUIC with NAT traversal).

```text
crates/server/          Rust backend, probe binary, and integration tests
  prompts/             Server-owned assistant prompt
crates/quic-client/     Shared iroh QUIC client
crates/android-bridge/  Rust JNI bridge for Android
android/               Native Kotlin/Compose app and Android tests
scripts/               Local development and test helpers
```

## Local setup

Requires Rust 1.93+ and the sibling `../flashmind` repository.

```sh
cp config.example.toml config.toml
# Edit config.toml: set your model and provider key env var.
cargo run -p beep-server --bin beep-server -- config.toml
```

The server prints its iroh node ID on startup. Use this ID to connect from
the Android app or the probe. No TLS certificates or port forwarding needed:
iroh handles NAT traversal through relay servers.

There are no accounts to configure. Each client signs in with its own iroh
key, and a key the server has not seen before becomes a new user, shown with
a name read from the key such as `mega-fox-trot`. Anyone who knows the node
ID can make an account. The Android app keeps its key in Google Block Store,
end-to-end encrypted, so a new phone on the same Google account signs back
in. Without that backup the user can save the key to a file from Settings.

The server keeps its data in Postgres with the pgvector extension. Set
`database_url` in the config or `DATABASE_URL` in the environment;
`just test-db` starts a local one in Docker.

## Test the endpoint

```sh
export BEEP_IDENTITY=.dev/probe-key  # made on first use
cargo run -q -p beep-server --bin beep-probe -- NODE_ID health
```

## Development server

```sh
python3 scripts/dev-server.py \
  --openrouter-key-file .dev/openrouter-key --model z-ai/glm-5.3-flash
```

The launcher writes an iroh secret key to `.dev/iroh-key`.
Without `--openrouter-key-file` it starts a scripted test provider instead.

## Wire protocol

The iroh transport uses ALPN `beep/2`. The client's iroh key is its identity,
so there is no login step. Each request gets its own bi stream of
length-delimited frames: a tag byte, then a msgpack body. See
`crates/proto/src/wire.rs`.

## Validation

```sh
cargo fmt -p beep-server --check
cargo clippy -p beep-server -- -D warnings
cargo test -q -p beep-server
```

Integration tests open real loopback iroh connections with a deterministic
model provider. They exercise streaming through Flashmind, memory tool
execution, reconnect/replay, idempotency, cancellation, user isolation, and
persistence/recovery. They do not require a live model account.

## OpenRouter for testing

Keep a raw key in `.dev/openrouter-key` (the `.dev/` directory is gitignored),
permissions `0600`. The launcher reads it into the server environment; it never
writes it into config files or client profiles. The default model is
`z-ai/glm-5.3-flash`.

## Firecrawl web search

Keep a raw key in `.dev/firecrawl-key` with permissions `0600`, then pass
`--firecrawl-key-file .dev/firecrawl-key` to the dev launcher. Without this
the search tool is unavailable.
