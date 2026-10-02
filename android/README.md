# Beep Android

Native Kotlin/Jetpack Compose client for the Beep Rust assistant. Android 11+
(API 30), currently built for **arm64-v8a** phones and Apple Silicon emulators.

## Features

- Full-screen chat with streaming text, tool activity, Stop, and retry.
- Multiple conversations and local history restored when the app reopens.
- Android conversation bubble: tap **Float chat**, allow notifications, and enable
  Beep bubbles in Android settings. The same chat UI opens above other apps.
- Server connection screen with node ID or JSON profile import.
- App token encrypted using an Android Keystore AES-GCM key. Model keys and prompts
  never enter the app. Chat cache is in app-private storage; backups and device
  transfer of the cache/credentials are disabled.
- P2P QUIC via iroh through the shared Rust client and a small JNI bridge.
  NAT traversal is handled by iroh relay servers. No TLS certificate setup needed.
- Durable request IDs and event cursors: retries reuse the original submission;
  partial text and its cursor are saved together so replay does not duplicate text.

## Structure

```text
android/app/                 Compose UI, local persistence, bubbles, tests
crates/quic-client/          Shared iroh QUIC client (Android and CLI probe)
crates/android-bridge/       JNI request/cancellation bridge; no agent/model code
crates/server/               Flashmind assistant, memory, runs, iroh listener
```

The transport uses iroh for P2P QUIC with built-in NAT traversal. The native
bridge currently opens a connection per request; connection pooling is a later
optimization. Streaming subscriptions last about 30 seconds and are resumed
from their cursor. The server keeps running when the app closes.

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

Enter the server's **node ID** (printed at startup) and your **Beep app token**
(leave it empty for testing-mode servers).

Alternatively import a JSON connection profile:

```json
{
  "node_id": "YOUR_SERVER_NODE_ID",
  "token": "YOUR_BEEP_APP_TOKEN"
}
```

Profiles contain an access credential: transfer them privately and remove the
plaintext copy from Downloads after import. Never include model-provider keys.
Changing the server/account clears the local conversation cache to avoid mixing
accounts. It does not delete server data.

## Development server

From the repository root:

```sh
python3 scripts/dev-server.py
```

This starts a **deterministic test provider**, not a real language model,
together with the Rust server. Every generated reply says "Test provider".
The server node ID is printed to the log at `.dev/android-server.log`.

For a real model:

```sh
python3 scripts/dev-server.py \
  --openrouter-key-file .dev/openrouter-key --model z-ai/glm-5.3-flash
```

Stop with Ctrl-C. Do not run multiple copies against the same database.

## Tests

```sh
cd android
./gradlew :app:testDebugUnitTest :app:lintDebug :app:assembleAndroidTest
```

The unit tests split UTF-8 and NDJSON across arbitrary byte boundaries and
verify truncated streams are rejected for replay. The instrumented test uses
real iroh QUIC to the local server and real Flashmind memory tools with the
deterministic provider, verifies memory recall in another conversation, and
opens the actual Android bubble activity.

With the development server running and an emulator connected:

```sh
adb push ../.dev/android-assets/testing-profile.json /sdcard/Download/beep-test.json
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb install -r app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb shell settings put secure notification_bubbles 1
adb shell am instrument -w -r \
  -e class com.flashmind.beep.ChatFlowTest \
  com.flashmind.beep.test/androidx.test.runner.AndroidJUnitRunner
adb shell rm /sdcard/Download/beep-test.json
```

Run instrumentation only on a development device: it resets Beep's local
chat cache and enables its notification/bubble permission for the test. It does
not reset server memory.

## UI

The bubble has a minimal toolbar (expand and an options menu), a short empty-state
prompt, and the composer. New conversation, history, and connection settings live
in the options menu. Long-press selection supports copying.
The full app adds conversation filtering and copy-reply controls. Both views render
a small Markdown subset (headings, bullets, bold, inline code, and HTTP(S) links).
Reading older messages pauses automatic scrolling; "Latest reply" resumes it.

The current app version is `0.1.0`. Use patch increments for subsequent releases
(`0.1.1`, `0.1.2`, ...). Android's internal `versionCode` must always increase so
updates install over existing builds, even when resetting the displayed version.
