#!/bin/sh
# Launch an Android emulator, build and install the debug APK, then tail logcat.
# Usage: ./scripts/emulator-test.sh [AVD_NAME]
set -eu
cd "$(dirname "$0")/.."

AVD="${1:-Pixel_7}"
ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
EMULATOR="$ANDROID_HOME/emulator/emulator"
ADB="$ANDROID_HOME/platform-tools/adb"

# Start the emulator if not already running.
if ! "$ADB" devices 2>/dev/null | grep -q "emulator-"; then
    echo "Starting emulator: $AVD"
    "$EMULATOR" -avd "$AVD" -no-snapshot-save -gpu auto &
    EMULATOR_PID=$!
    echo "Waiting for device to boot…"
    "$ADB" wait-for-device
    # Wait for boot to complete.
    while [ "$("$ADB" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" != "1" ]; do
        sleep 2
    done
    echo "Emulator booted."
else
    echo "Emulator already running."
fi

# Ensure the dev server has generated profiles.
if [ ! -f .dev/android-emulator.json ]; then
    echo "ERROR: .dev/android-emulator.json not found. Run 'python3 scripts/dev-server.py' first." >&2
    exit 1
fi

# Copy emulator profile to assets for debug builds.
mkdir -p .dev/android-assets
cp .dev/android-emulator.json .dev/android-assets/testing-profile.json

# Build and install the debug APK.
echo "Building debug APK…"
cd android
./gradlew installDebug -q
cd ..

echo "Launching app…"
"$ADB" shell am start -n com.flashmind.neobeep/.MainActivity

echo "Tailing logcat (Ctrl+C to stop)…"
"$ADB" logcat -s "neobeep:*" "NativeQuic:*" "BeepApp:*" "AndroidRuntime:E" | grep --line-buffered -v "^$"
