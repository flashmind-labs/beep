# Agent notes

## Getting the app onto the phone

Install the release APK on the physical phone over wireless ADB, and the debug
APK on an emulator (`./gradlew assembleDebug`). The phone's ADB port changes, so
take it from `adb devices`.

    cd apps/android && ./gradlew assembleRelease
    adb -s sm-f966b.box.ray:<port> install -r apps/android/app/build/outputs/apk/release/app-release.apk

If the phone is not on ADB, send the same APK with
`ray send sm-f966b.box.ray <apk>`, which queues it while the phone is offline.
Never serve the file over HTTP.

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

## iOS app

`apps/ios` is the SwiftUI port. `just ios-run` builds the Rust bridge for the
simulator, generates the Xcode project with xcodegen and launches it on the
iPhone 17 simulator. Debug builds take `-send "text"` and `-open
chats|settings|memories|routines` as launch arguments
(`xcrun simctl launch booted com.flashmind.beep -send "hi"`), so it can be
tested without tapping.
