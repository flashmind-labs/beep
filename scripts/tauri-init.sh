#!/usr/bin/env bash
# Generates the phone app's native project (apps/desktop/src-tauri/gen/apple
# or gen/android) when it is missing. The generated projects are not committed.
set -euo pipefail

platform=${1:?usage: tauri-init.sh ios|android}
cd "$(dirname "$0")/../apps/desktop"
case "$platform" in
    ios) dir=src-tauri/gen/apple; made=$dir/project.yml ;;
    android) dir=src-tauri/gen/android; made=$dir/app/build.gradle.kts ;;
    *) echo "usage: tauri-init.sh ios|android" >&2; exit 1 ;;
esac
# A folder without its project (left by a failed or partial run) is made again.
[[ -f $made ]] && exit 0
rm -rf "$dir"

[[ -d node_modules ]] || npm ci --silent
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
