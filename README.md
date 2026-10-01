# Neobeep

An Android bubble-chat assistant backed by a Rust server using Flashmind.
The server owns model credentials, prompts, tools, conversations, and memory.
The app connects over **HTTP/3 over QUIC**, with TLS certificate validation.

```text
crates/server/          Rust backend, QUIC probe, and integration tests
  prompts/             Server-owned assistant prompt inspired by MrBeep
crates/quic-client/     Shared HTTP/3 client
crates/android-bridge/  Rust JNI bridge for Android
android/               Native Kotlin/Compose app and Android tests
scripts/               Local development and test helpers
```

## Implemented

- Real HTTP/3 over UDP using Quinn, rustls, h3, and h3-quinn. No TCP fallback.
- Flashmind agent runtime and streaming with an OpenAI-compatible model endpoint.
- Per-user bearer authentication; tokens and provider keys are loaded from environment variables.
- SQLite conversations and durable, sequenced run events, isolated by authenticated user.
- Server-owned runs that continue after a client disconnects; replay using an event cursor.
- Idempotent message submission; concurrent turns within one session are rejected.
- Cancellation, a 180-second run deadline, 20 tool starts per run, and 16 concurrent runs.
- Scoped `memory_store`, `memory_recall`, `memory_forget`, and `get_time` tools.
- MrBeep-inspired tone, progressive conversational onboarding, and memory behavior.
- Startup recovery marks unfinished runs interrupted, without automatically repeating tool actions.
- Android chat, system bubble, multiple local conversations, streaming/replay,
  cancellation, encrypted app tokens, and private connection-profile import.

See [Android setup and testing](android/README.md) for the APK, emulator, and
Rayfish workflow.

This is the first working server slice, not full MrBeep parity. Memory currently
uses literal substring recall in SQLite, not Flashmind's vector/embedding store.
Automatic fact extraction, evolving personality, integrations, approvals, reminders,
push delivery, and attachments are follow-up work. No live provider
credentials are included. System prompts are not returned by the history API;
this is not a guarantee against a model paraphrasing its instructions.

## Local setup

Requires Rust 1.93+, OpenSSL, and the sibling `../flashmind` repository.
The server deliberately uses a local path dependency while we develop both projects.

```sh
sh scripts/dev-cert.sh
cp config.example.toml config.toml
export NEOBEEP_TOKEN="$(openssl rand -hex 32)"
# Edit config.toml: set your provider_url and model (openai:<model-name>).
# For a provider requiring a key, enable provider_key_env and set that variable.
cargo run -p neobeep-server --bin neobeep-server -- config.toml
```

Keep the same client token for future starts using your secret manager or a local,
ignored environment file. Do not use the model provider's key as the app token.
Each configured user has a distinct random token of at least 32 characters.
This initial authentication is provisioned manually; login/device enrollment and
token rotation UI are not implemented. Run one server process per database.

The sample binds to `127.0.0.1:4433` over **UDP**. For a remote deployment, bind to
the intended interface, permit inbound UDP on that port, and supply a certificate
valid for the server hostname. The generated certificate is for local testing only.
Protect the database and private key with filesystem permissions appropriate to
the service account. There is no automatic event retention/cleanup yet.

## Test the actual QUIC endpoint

The probe validates the provided CA and speaks only HTTP/3, so success cannot
silently mean HTTP/2 or TCP. Use the same `NEOBEEP_TOKEN` as the server.

```sh
cargo run -q -p neobeep-server --bin neobeep-probe -- \
  127.0.0.1:4433 localhost .dev/localhost.pem GET /v1/health
```

Create `.dev/message.json` with a new request UUID for each distinct message:

```json
{
  "session_id": "78df6da7-e70a-47d5-b1ad-a7a89759fb9b",
  "request_id": "69a013b0-71e8-4f34-9a48-0f167877ea4a",
  "message": "Remember that I prefer concise replies."
}
```

```sh
cargo run -q -p neobeep-server --bin neobeep-probe -- \
  127.0.0.1:4433 localhost .dev/localhost.pem POST /v1/runs .dev/message.json
# Replace RUN_ID with the returned id:
cargo run -q -p neobeep-server --bin neobeep-probe -- \
  127.0.0.1:4433 localhost .dev/localhost.pem GET '/v1/runs/RUN_ID/events?after=0'
```

## Protocol v1

Outside explicit testing mode, API endpoints except `/v1/auth` require
`Authorization: Bearer <app-token>`. JSON responses are
marked `no-store`. User identity comes only from authentication, never request bodies.

| Method | Path | Behavior |
| --- | --- | --- |
| GET | `/v1/health` | Authenticated transport health |
| POST | `/v1/runs` | Submit `{session_id, request_id, message}`; UUID identifiers |
| GET | `/v1/runs/{id}` | Run status |
| GET | `/v1/runs/{id}/events?after=N` | Stream/replay newline-delimited JSON |
| POST | `/v1/runs/{id}/cancel` | Request cancellation; observe terminal status in events |
| GET | `/v1/sessions` | List up to 200 saved conversations belonging to the authenticated user |
| GET | `/v1/sessions/{id}` | Saved user/assistant text history, without internal prompts/reasoning |

New submissions return 202; an identical retry returns 200 and the same run ID.
Reusing a request ID with a different message/session, sending into a busy session,
or exceeding run capacity returns 409. Invalid inputs return 400, unknown or
other-user resources 404, and invalid authentication 401. Bodies are limited to
64 KiB and message text to 32 KiB. Cancel does not undo tools already completed.

Events have `run_id`, monotonically increasing `seq`, and a `type`:
`run_started`, `text_delta`, `tool_started`, `tool_result`, `usage`, `error`,
or `run_finished`. Terminal statuses are `completed`, `cancelled`, `failed`,
`timed_out`, `tool_limit`, or `interrupted`.

Subscriptions close on completion or after approximately 30 seconds. EOF alone
does not mean the run completed. Reconnect with the last processed sequence,
deduplicate by `(run_id, seq)`, and check status if necessary. NDJSON lines can be
split across arbitrary network chunks. A disconnect never automatically retries
a tool. A process restart preserves events and completed sessions, but interrupts
unfinished runs; partial conversation state from that run is not restored.

## Validation

```sh
cargo fmt --all --check
cargo test -q --workspace
cargo clippy -q --workspace --all-targets -- -D warnings
```

Integration tests open real loopback QUIC connections with verified test
certificates and a deterministic model provider. They exercise streaming through
Flashmind, memory tool execution, reconnect/replay, idempotency, cancellation,
user isolation, persistence/recovery, and rejection of untrusted certificates.
They do not require or validate a live model account.

## Next milestones

1. Connect the Android client to a production model and verify a physical Rayfish phone.
2. Memory inspection UI, vector recall, and MrBeep-style capture/personality jobs.
3. Integrations with durable approvals, timezone-aware reminders, and push delivery.
4. Device enrollment, operational limits/retention, and remote deployment hardening.

Implementation references: [Flashmind](../flashmind/README.md),
[MrBeep](../old/mrbeep/README.md), [h3](https://github.com/hyperium/h3),
[Quinn](https://docs.rs/quinn/latest/quinn/), and
[Android Cronet](https://developer.android.com/develop/connectivity/cronet).

## Browser chat

Set `web_enabled = true` in server configuration (the development launcher does
this automatically). Open **http://localhost:4433/** on the server computer, or
**http://build-box.example:4433/** from a device connected to the same Rayfish
network. The development launcher enables credential-free testing; the page opens
directly into a shared development account. With testing mode disabled, the
browser asks for the existing app token. No model keys or
tokens are embedded in the webpage.

The browser uses HTTP over TCP; Android continues to use HTTP/3 over QUIC/UDP.
Both listeners use the same configured addresses and port, authentication,
Flashmind agent, SQLite conversations, memories, and run events. Allow TCP 4433
on the private Rayfish network as well as the existing UDP rule:

```sh
ray firewall add in allow --proto tcp --port 4433 --network box
```

The webpage uses plain HTTP over the private Rayfish VPN, with no browser
certificate setup. The Android QUIC transport retains its TLS configuration.
Rayfish supplies the hostname `build-box.example` for this server.

The responsive web client supports streaming, cancellation, reconnect/replay,
and loading saved Android conversations. Its token and cached state live in
sessionStorage for that browser tab; disconnect clears both. Server history is
loaded again after signing in. Android also discovers server conversations on connection and when opening its
conversation drawer, and refreshes saved messages when selecting a conversation.

Assets in `web/` are embedded in the Rust binary; rebuild/restart the server after
editing them. Only the four explicit static asset paths are served. QR device
pairing remains a later step. The development launcher still uses the clearly
labeled deterministic test provider, not a production model.

```sh
node --check web/app.js
node --test web/protocol.test.mjs
cargo test -q -p neobeep-server --tests
```

The server integration tests also exercise HTTP, unauthenticated
rejection, static-file allowlisting, conversation isolation, and a browser-created
conversation read through the Android QUIC transport.

## Testing authentication

The development launcher explicitly sets `testing_user = "development"`. Both
HTTP and QUIC requests then use that shared account without credentials. Existing
Android tokens continue working against the same account. Anyone who can reach
this test server can read and change its conversations and memory.

Omit `testing_user` to require authentication (the default). There is no automatic
fallback to anonymous access on a bad token or an authentication error. `GET
/v1/auth` publicly reports only the authentication mode, so the browser knows
whether to show sign-in. Production passkey registration/sign-in is planned but
not implemented; until then non-testing servers require bearer tokens.

## OpenRouter for testing

The server uses Flashmind's native `OpenRouterProvider` for `openrouter:` models.
Keep a raw testing key in a private file, for example `.dev/openrouter-key` (the
entire `.dev/` directory is ignored by Git), with file permissions `0600`. Start:

```sh
python3 scripts/dev-server.py --rayfish \
  --openrouter-key-file .dev/openrouter-key --model PROVIDER/MODEL_ID
```

The launcher reads the key into the server's `OPENROUTER_API_KEY` environment;
it never writes it into TOML, browser assets, or Android profiles. This mode does
not start the scripted provider and does not fall back to it if OpenRouter fails.
The default model is `z-ai/glm-5.3-flash`; use `--model` to override it. Existing testing-mode authentication
and shared conversations remain in effect.


## Firecrawl web search

Keep a raw Firecrawl key in `.dev/firecrawl-key` with permissions `0600`, then run:

```sh
python3 scripts/dev-server.py --rayfish \
  --openrouter-key-file .dev/openrouter-key \
  --firecrawl-key-file .dev/firecrawl-key
```

The launcher passes the key only through the server environment. For other
launchers, set `firecrawl_key_env = "FIRECRAWL_API_KEY"` in the server config.
Without this setting the search tool is unavailable. `web_search` uses Firecrawl's
v2 search API with page excerpts, defaults to three results (maximum five), and
returns source URLs for citations. Requests have a 45-second timeout and bounded
response/content sizes; errors omit upstream response bodies and credentials.
Both clients show search progress. Search runs only within an assistant turn,
and Stop cancels the active tool through Flashmind's cancellation token.
