# Agent notes

## Getting the app onto the phone

Build the release APK and send it over Rayfish. Do not send the debug APK and
do not serve the file over HTTP.

    cd android && ./gradlew assembleRelease
    ray send sm-f966b.box.ray android/app/build/outputs/apk/release/app-release.apk

`ray send` queues the file if the phone is offline.

## Server tests

`just test` starts the local Postgres and runs the server tests. A plain
`cargo test -p beep-server` fails without `BEEP_TEST_DATABASE_URL`.
