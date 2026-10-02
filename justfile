server_config := ".dev/android-server.toml"
apk := "android/app/build/outputs/apk/debug/app-debug.apk"

# Build and deploy everything: server + Android APK
deploy: build-server restart-server bake-profile build-android install-apk launch-app

# Build the server
build-server:
    cargo build --package beep-server

# Build the Android debug APK
build-android:
    cd android && ./gradlew assembleDebug

# Restart the server (kill old, start new)
restart-server:
    -pkill -f "beep-server {{server_config}}"
    @sleep 1
    bash -c 'target/debug/beep-server {{server_config}} > .dev/server.log 2>&1 & disown'
    @sleep 2
    @pgrep -f "beep-server {{server_config}}" > /dev/null && echo "Server started" || (echo "Server failed to start, check .dev/server.log"; exit 1)

# Write the server's node ID into the debug asset profile
bake-profile:
    #!/usr/bin/env bash
    node_id=$(sed 's/\x1b\[[0-9;]*m//g' .dev/server.log | grep -o 'node_id=[0-9a-f]*' | tail -1 | cut -d= -f2)
    if [ -z "$node_id" ]; then echo "No node ID in server log"; exit 1; fi
    printf '{"node_id":"%s"}\n' "$node_id" > android/app/src/main/assets/testing-profile.json
    echo "Baked node ID: ${node_id:0:12}..."

# Install APK on connected device
install-apk:
    adb install -r {{apk}}

# Launch the app on device
launch-app:
    adb shell am start -n com.flashmind.beep/.MainActivity

# Server only: rebuild and restart
server: build-server restart-server

# Android only: rebuild, install, launch
android: bake-profile build-android install-apk launch-app

# Tail server logs
logs:
    tail -f .dev/server.log

# Tail device logcat for Beep
logcat:
    adb logcat -s beep,BeepApplication,MainActivity --format=brief
