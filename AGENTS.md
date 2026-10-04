# Agent notes

## Getting the app onto the phone

Install the release APK on the physical phone over wireless ADB, and the debug
APK on an emulator (`./gradlew assembleDebug`). The phone's ADB port changes, so
take it from `adb devices`.

    cd android && ./gradlew assembleRelease
    adb -s phone.example:<port> install -r android/app/build/outputs/apk/release/app-release.apk

If the phone is not on ADB, send the same APK with
`ray send phone.example <apk>`, which queues it while the phone is offline.
Never serve the file over HTTP.

## Server tests

`just test` starts the local Postgres and runs the server tests. A plain
`cargo test -p beep-server` fails without `BEEP_TEST_DATABASE_URL`.
