# Agent notes

## Getting the app onto the phone

Releases always go to Play internal testing. Every push to main runs the `apk`
workflow, which signs the bundle with the upload key and uploads it to the
internal track, and the phone updates from Play. Do not hand-install APKs on
the phone: a locally built APK is signed with another key and will not install
over the Play build.

Use the debug APK on an emulator (`./gradlew assembleDebug`).

## UI changes

Every screen of the app is mocked in the Beep screens artifact:
https://claude.ai/artifact/BwYePcE9BHvLcYSvkRwvxD

Before changing the Android UI, update the mock first so it shows the new
screen or state, then change the app to match.

## Server tests

`just test` starts the local Postgres and runs the server tests. A plain
`cargo test -p beep-server` fails without `BEEP_TEST_DATABASE_URL`.

## Desktop app

`apps/desktop` is the macOS tray app (Tauri 2, its own Cargo workspace). Run
it with `just desktop-dev`, check it with `just desktop-check`, and build the
DMG with `just desktop-build` (it lands in `target/beep-macos.dmg`; the
installer window comes from `apps/desktop/installer/`). The `desktop` Forgejo
workflow builds the same DMG on the Mac runner. Its key and state live in
`~/Library/Application Support/com.flashmind.beep.desktop/`. The bar's screens
are in the same mock as the phone's.

To release it, `just desktop-release 0.2.0` sets the version in
`apps/desktop/src-tauri/Cargo.toml`, commits and tags `desktop-v0.2.0`. Pushing
the tag runs the same workflow, which refuses to build without the Developer ID
and notary secrets, then attaches a signed, notarized `Beep-0.2.0.dmg` to the
tag's Forgejo release.

## iOS app

`apps/ios` is the SwiftUI port. `just ios-run` builds the Rust bridge for the
simulator, generates the Xcode project with xcodegen and launches it on the
iPhone 17 simulator. Debug builds take `-send "text"` and `-open
chats|settings|memories|routines` as launch arguments
(`xcrun simctl launch booted com.flashmind.beep -send "hi"`), so it can be
tested without tapping.
