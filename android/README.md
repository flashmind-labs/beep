# Neobeep Android

Native Kotlin/Jetpack Compose client for the Neobeep Rust assistant. Android 11+
(API 30), currently built for **arm64-v8a** phones and Apple Silicon emulators.

## Features

- Full-screen chat with streaming text, tool activity, Stop, and retry.
- Multiple conversations and local history restored when the app reopens.
- Android conversation bubble: tap **Float chat**, allow notifications, and enable
  Neobeep bubbles in Android settings. The same chat UI opens above other apps.
- Server connection screen with manual configuration or JSON profile import.
- App token encrypted using an Android Keystore AES-GCM key. Model keys and prompts
  never enter the app. Chat cache is in app-private storage; backups and device
  transfer of the cache/credentials are disabled.
- Genuine HTTP/3 over QUIC via the shared Rust client and a small JNI bridge.
  TLS verification is always enabled; public roots and optional private CA PEMs
  are supported. There is no silent TCP fallback or trust-all mode.
- Durable request IDs and event cursors: retries reuse the original submission;
  partial text and its cursor are saved together so replay does not duplicate text.

## Structure

```text
android/app/                 Compose UI, local persistence, bubbles, tests
crates/quic-client/          Shared HTTP/3 client (Android and CLI probe)
crates/android-bridge/       JNI request/cancellation bridge; no agent/model code
crates/server/               Flashmind assistant, memory, runs, QUIC listener
```

The transport uses Quinn/rustls rather than Cronet. This reuses the tested Rust
client and permits explicit private-CA configuration for a server on Rayfish.
The native bridge currently opens a connection per HTTP request; connection
pooling is a later optimization. Streaming subscriptions last about 30 seconds
and are resumed from their cursor. The server keeps running when the app closes.

## Build

Install JDK 17 or 21, SDK platform/build tools 36, NDK `28.2.13676358`, and Rust:

```sh
rustup target add aarch64-linux-android
# Set ANDROID_HOME or use android/local.properties with sdk.dir=...
cd android
./gradlew :app:assembleDebug
```

Gradle invokes `../scripts/build-android-native.sh` before assembling the app.
Use Android Studio's bundled JDK if your default Java version is incompatible.
The native build script finds Cargo in `~/.cargo/bin` as well as PATH.

APK: `app/build/outputs/apk/debug/app-debug.apk` (development signing).

```sh
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

## Connect to your server

Enter the server's **host:port**, its **TLS certificate hostname**, and your
**Neobeep app token** (leave it empty for testing-mode servers). For a private certificate, import its PEM file. The
hostname must match the certificate even when the connection address is a
Rayfish IP. The app does not accept arbitrary certificates merely because a VPN
is connected.

Alternatively import a JSON connection profile:

```json
{
  "address": "[YOUR_RAYFISH_IPV6]:4433",
  "hostname": "your-certificate-hostname",
  "token": "YOUR_NEOBEEP_APP_TOKEN",
  "ca": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n"
}
```

`ca` can be omitted for a certificate chaining to a supported public root.
Profiles contain an access credential: transfer them privately and remove the
plaintext copy from Downloads after import. Never include model-provider keys.
Changing the server/account clears the local conversation cache to avoid mixing
accounts. It does not delete server data. Server-side conversation discovery and
history import across fresh device installations are not implemented yet.

## Local emulator and Rayfish testing

From the repository root:

```sh
python3 scripts/dev-server.py --rayfish
```

This explicitly starts a **deterministic test provider**, not a real language
model, together with the Rust server. Every generated reply says “Test provider”.
It generates private profiles at:

- `.dev/android-emulator.json`: `10.0.2.2:4433` for the emulator's host gateway.
- `.dev/android-rayfish.json`: the Mac's Rayfish IPv6 address for VPN peers.

The server binds to loopback and the first active Rayfish network's local IP,
not every LAN/public interface. The helper does not change VPN firewall rules.
If your Rayfish firewall defaults to deny, allow just this port on the intended
network:

```sh
/Applications/Rayfish.app/Contents/MacOS/ray firewall add in allow \
  --proto udp --port 4433 --network YOUR_NETWORK
```

Run Rayfish on the phone and join the same authorized network before using the
Rayfish profile. The helper's local certificate uses `localhost` as the TLS
hostname and the profile includes that certificate as a trust anchor. The
connection address remains the VPN IP. Production certificates should use the
intended server hostname. Local development certificates expire after 30 days.

Stop the helper with Ctrl-C; it stops both child processes. Do not run multiple
copies against the same development database/ports. Omit `--rayfish` for purely
local emulator testing. For a real assistant, run the server with your chosen
Flashmind model/provider configuration instead of the fixture.

## Tests

```sh
cd android
./gradlew :app:testDebugUnitTest :app:lintDebug :app:assembleAndroidTest
```

The unit tests split UTF-8 and NDJSON across arbitrary byte boundaries and
verify truncated streams are rejected for replay. The instrumented test uses
real QUIC to the local server and real Flashmind memory tools with the deterministic
provider, verifies memory recall in another conversation, and opens the actual
Android bubble activity.

With the development helper running and an emulator connected:

```sh
adb push ../.dev/android-emulator.json /sdcard/Download/neobeep-test.json
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb install -r app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb shell settings put secure notification_bubbles 1
adb shell am instrument -w -r \
  -e class com.flashmind.neobeep.ChatFlowTest \
  com.flashmind.neobeep.test/androidx.test.runner.AndroidJUnitRunner
adb shell rm /sdcard/Download/neobeep-test.json
```

Run instrumentation only on a development device: it resets Neobeep's local
chat cache and enables its notification/bubble permission for the test. It does
not reset server memory. Shell access to `ray` on this Mac and binding the server
are not proof that an offline phone can reach it; verify from a connected peer.

## Remaining work

Production model configuration, remote phone verification, rich Markdown/media,
device enrollment and push notifications. System
bubbles depend on the user's Android settings and device support. There is no
overlay permission, accessibility automation, or permanent background service.


## Current testing build

The debug APK reads a token-free `testing-profile.json` from
`.dev/android-assets/` when no connection settings have been saved. Run
`scripts/dev-server.py --rayfish --openrouter-key-file .dev/openrouter-key`
before building to generate it. It contains only the Rayfish DNS address,
TLS hostname, and public development certificate. Release builds do not include
this directory. Existing saved settings take precedence.

Testing mode opens directly into chat, sharing server history/memory with the web.
Conversations refresh when connecting or opening the drawer; selecting a saved
conversation fetches its latest messages. Active local runs keep their durable
cursor and partial text. Model selection is server-side; the default is now
OpenRouter `z-ai/glm-5.3-flash`.

For live-model instrumentation, push a token-free emulator connection profile to
`/sdcard/Download/neobeep-test.json` and pass `-e liveModel true` to
`am instrument`. The live flow checks arithmetic replies over QUIC, a fresh
conversation, and opening the actual Android bubble.


The bubble requests a 440dp expanded height (Android decides its final
size), with a 48dp toolbar, smaller message spacing, a three-line composer,
and a compact welcome screen. Connection settings remain in the conversation
drawer. HTTP(S) links and Markdown source links are tappable in both chat views.
Enable server-side Firecrawl with `--firecrawl-key-file .dev/firecrawl-key`;
no search credential is included in the APK.


### Current UI

The bubble has a minimal toolbar (expand and an options menu), a short empty-state
prompt, and the composer. New conversation, history, and connection settings live
in the options menu. It no longer shows suggestion cards, copy action rows, or an
explanatory dialog every time it opens. Long-press selection still supports copying.
The full app adds conversation filtering and copy-reply controls. Both views render
a small Markdown subset (headings, bullets, bold, inline code, and HTTP(S) links).
Reading older messages pauses automatic scrolling; “Latest reply” resumes it.

The current app version is `0.1.0`. Use patch increments for subsequent releases
(`0.1.1`, `0.1.2`, …). Android's internal `versionCode` must always increase so
updates install over existing builds, even when resetting the displayed version.
