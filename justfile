set dotenv-load

server_config := ".dev/android-server.toml"
apk := "android/app/build/outputs/apk/debug/app-debug.apk"

# Build and deploy everything: server + Android APK
deploy: build-server build-android restart-server install-apk launch-app

# Build the server
build-server:
    cargo build --package neobeep-server

# Build the Android debug APK
build-android:
    cd android && ./gradlew assembleDebug

# Restart the server (kill old, start new)
restart-server:
    -pkill -f "neobeep-server {{server_config}}"
    @sleep 1
    bash -c 'target/debug/neobeep-server {{server_config}} > .dev/server.log 2>&1 & disown'
    @sleep 2
    @pgrep -f "neobeep-server {{server_config}}" > /dev/null && echo "Server started" || (echo "Server failed to start — check .dev/server.log"; exit 1)

# Install APK on connected device
install-apk:
    adb install -r {{apk}}

# Launch the app on device
launch-app:
    adb shell am start -n com.flashmind.neobeep/.MainActivity

# Server only: rebuild and restart
server: build-server restart-server

# Android only: rebuild, install, launch
android: build-android install-apk launch-app

# Tail server logs
logs:
    tail -f .dev/server.log

# Tail device logcat for Beep
logcat:
    adb logcat -s neobeep,BeepApplication,MainActivity --format=brief
