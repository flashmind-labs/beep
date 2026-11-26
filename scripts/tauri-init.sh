#!/usr/bin/env bash
# Generates the phone app's native project (apps/desktop/src-tauri/gen/apple
# or gen/android) when it is missing, then puts Beep's icon in it. The
# generated projects are not committed.
set -euo pipefail

platform=${1:?usage: tauri-init.sh ios|android}
cd "$(dirname "$0")/../apps/desktop"
case "$platform" in
    ios) dir=src-tauri/gen/apple; made=$dir/project.yml ;;
    android) dir=src-tauri/gen/android; made=$dir/app/build.gradle.kts ;;
    *) echo "usage: tauri-init.sh ios|android" >&2; exit 1 ;;
esac

[[ -d node_modules ]] || npm ci --silent
# A folder without its project (left by a failed or partial run) is made again.
if [[ ! -f $made ]]; then
    rm -rf "$dir"

    if [[ $platform == ios ]]; then
        # `tauri ios init` tries to brew-install CocoaPods and libimobiledevice,
        # which the build does not need. For init only, `pod` does nothing and
        # `brew` skips libimobiledevice and runs everything else.
        shims=$(mktemp -d)
        trap 'rm -rf "$shims"' EXIT
        real_brew=$(command -v brew || echo false)
        printf '#!/bin/sh\nexit 0\n' > "$shims/pod"
        printf '#!/bin/sh\nfor arg in "$@"; do case "$arg" in libimobiledevice) exit 0 ;; esac; done\nexec %s "$@"\n' "$real_brew" > "$shims/brew"
        chmod +x "$shims/pod" "$shims/brew"
        PATH="$shims:$PATH" npm run tauri -- ios init --ci
    else
        npm run tauri -- android init --ci
    fi
fi

# The phone icon is full-bleed: the system rounds its corners. Init leaves
# Tauri's own icon in the project, so ours replaces it on every run.
icons=$(mktemp -d)
npm run --silent tauri -- icon src-tauri/icons/phone.png -o "$icons" --ios-color '#0B0E1D' >/dev/null 2>&1

if [[ $platform == ios ]]; then
    cp "$icons"/ios/*.png "$dir/Assets.xcassets/AppIcon.appiconset/"

    # netdev, under iroh, reads the DNS servers through libresolv. The Rust
    # library cannot carry the link, so the app links it.
    if ! grep -q libresolv.tbd "$dir/project.yml"; then
        perl -0pi -e 's|^(\s*)- sdk: SystemConfiguration\.framework\n|$&$1- sdk: libresolv.tbd\n|m' "$dir/project.yml"
        (cd "$dir" && xcodegen generate --quiet)
    fi
else
    sed -i.bak 's/#[0-9A-Fa-f]\{6\}/#0B0E1D/' "$icons/android/values/ic_launcher_background.xml"
    rm "$icons/android/values/ic_launcher_background.xml.bak"
    cp -R "$icons"/android/. "$dir/app/src/main/res/"

    # iroh reads the network state to find DNS servers and follow network
    # changes; without it a phone keeps paths that died when it switched.
    manifest=$dir/app/src/main/AndroidManifest.xml
    grep -q ACCESS_NETWORK_STATE "$manifest" ||
        perl -0pi -e 's|(\s*)(<uses-permission android:name="android.permission.INTERNET"\s*/>)|$1$2$1<uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />|' "$manifest"
fi

rm -rf "$icons"
