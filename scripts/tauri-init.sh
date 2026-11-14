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
else
    sed -i.bak 's/#[0-9A-Fa-f]\{6\}/#0B0E1D/' "$icons/android/values/ic_launcher_background.xml"
    rm "$icons/android/values/ic_launcher_background.xml.bak"
    cp -R "$icons"/android/. "$dir/app/src/main/res/"
    # The release build reads every .pro file in the app folder.
    cp src-tauri/android-rules.pro "$dir/app/beep-rules.pro"
fi

rm -rf "$icons"
