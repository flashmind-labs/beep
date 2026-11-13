set dotenv-load

server_config := ".dev/android-server.toml"

# Serve the platform mockups on localhost
mockups port="8000":
    python3 -m http.server {{port}} --bind 127.0.0.1 --directory mockups

# Build the server
build-server:
    cargo build --package beep-server

# Run the macOS desktop app from source
# The window only loads tauri://localhost, so dev serves the built bundle
# instead of a dev server; press Cmd+R after a frontend change.
desktop-dev:
    cd apps/desktop && npm ci --silent && npm run build
    cd apps/desktop/src-tauri && cargo tauri dev --no-dev-server

# Build the desktop DMG.
desktop-build:
    cd apps/desktop && npm ci --silent
    cd apps/desktop/src-tauri && PATH="/usr/bin:$PATH" cargo tauri build --bundles app
    bash scripts/package-macos-dmg.sh "${CARGO_TARGET_DIR:-apps/desktop/src-tauri/target}/release/bundle/macos/Beep.app" target/beep-macos.dmg

# Build the desktop DMG signed with a Developer ID from the keychain, with
# the native updater as the release workflow builds it.
desktop-build-signed:
    #!/usr/bin/env bash
    set -euo pipefail
    export APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-Developer ID Application}"
    if ! security find-identity -v -p codesigning | grep -qF -- "$APPLE_SIGNING_IDENTITY"; then
        echo "no \"$APPLE_SIGNING_IDENTITY\" signing identity in the keychain" >&2
        exit 1
    fi

    bash scripts/download-sparkle.sh
    bash scripts/sign-sparkle.sh
    (cd apps/desktop && npm ci --silent)
    (cd apps/desktop/src-tauri && PATH="/usr/bin:$PATH" cargo tauri build --bundles app --features sparkle --config tauri.sparkle.conf.json)

    app="${CARGO_TARGET_DIR:-apps/desktop/src-tauri/target}/release/bundle/macos/Beep.app"
    codesign --verify --deep --strict "$app"
    bash scripts/package-macos-dmg.sh "$app" target/beep-macos.dmg

# Tag HEAD server-v<version> and push the tag
server-release version:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "{{version}}" =~ ^0\.[0-9]+\.[0-9]+$ ]] || { echo "version must be like 0.2.3: every app stays on 0.x until production is ready" >&2; exit 1; }
    [[ $(git branch --show-current) == main ]] || { echo "release from main" >&2; exit 1; }
    [[ -z $(git status --porcelain) ]] || { echo "commit or stash your changes first" >&2; exit 1; }
    python3 scripts/changelog.py notes {{version}} --path changelogs/server.md > /dev/null
    git tag -a "server-v{{version}}" -m "Beep server {{version}}"
    git push origin "server-v{{version}}"
    git push forgejo "server-v{{version}}"

# Bump selected apps, commit together and publish main plus their release tags.
# Examples: just patch desktop; just patch desktop beep-node android ios; just patch all.
# Add --dry-run to fetch and check versions without creating a release.
[positional-arguments]
patch +apps:
    #!/usr/bin/env bash
    set -euo pipefail
    uv run --script scripts/release.py patch "$@"

# Bump selected apps to the next 0.x minor version and publish them.
[positional-arguments]
minor +apps:
    #!/usr/bin/env bash
    set -euo pipefail
    uv run --script scripts/release.py minor "$@"

# Release every app, or only the listed apps, with a patch bump.
[positional-arguments]
release *apps:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ $# == 0 ]]; then set -- all; fi
    uv run --script scripts/release.py patch "$@"

# Format, lint and test the desktop app
desktop-check:
    cd apps/desktop && npm ci --silent && npm run build && npm run check
    cd apps/desktop/src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test

# Build the phone app (the desktop app built for iOS) for the simulator
ios-build:
    rustup component add llvm-tools
    bash scripts/tauri-init.sh ios
    cd apps/desktop && npm ci --silent && npm run tauri -- ios build -v --debug --target aarch64-sim

# Build, install and launch the phone app on the iPhone 17 simulator
ios-sim-run:
    bash scripts/tauri-ios.sh --sim "iPhone 17"

# Build a release .ipa of the phone app, to install later with ios-install
ios-ipa:
    bash scripts/tauri-ios.sh --ipa

# Install the last built .ipa on a connected iPhone and open it. Optional device name.
[positional-arguments]
ios-install device="":
    #!/usr/bin/env bash
    set -euo pipefail
    ipa=$(ls -t apps/desktop/src-tauri/gen/apple/build/arm64/*.ipa 2>/dev/null | head -1)
    [[ -n $ipa ]] || { echo "No .ipa yet: run just ios-ipa" >&2; exit 1; }
    device="$1"
    if [[ -z $device ]]; then
        list=$(mktemp)
        xcrun devicectl list devices --json-output "$list" > /dev/null
        device=$(python3 -c 'import json,sys; ds=[d for d in json.load(open(sys.argv[1]))["result"]["devices"] if d.get("hardwareProperties",{}).get("platform")=="iOS" and d.get("hardwareProperties",{}).get("reality")=="physical"]; print(ds[0]["identifier"] if len(ds)==1 else "")' "$list")
        rm -f "$list"
    fi
    [[ -n $device ]] || { echo "Connect and unlock an iPhone, or pass its name" >&2; exit 1; }
    xcrun devicectl device install app --device "$device" "$ipa"
    xcrun devicectl device process launch --device "$device" com.flashmind.beep

# Build, install and launch the phone app on a connected iPhone. Optional device name.
[positional-arguments]
ios-run device="":
    just ios-ipa
    just ios-install "$1"

# Build the phone app's debug APK for arm64
android-build:
    bash scripts/tauri-init.sh android
    cd apps/desktop && npm ci --silent && npm run tauri -- android build -v --debug --apk --target aarch64

# Build, install and launch the phone app on a connected Android device or emulator
android-run:
    bash scripts/tauri-init.sh android
    cd apps/desktop && npm ci --silent && npm run tauri -- android run -v

# Restart the server (kill old, start new)
restart-server:
    -pkill -f "beep-server {{server_config}}"
    @sleep 1
    bash -c 'target/debug/beep-server {{server_config}} > .dev/server.log 2>&1 & disown'
    @sleep 2
    @pgrep -f "beep-server {{server_config}}" > /dev/null && echo "Server started" || (echo "Server failed to start, check .dev/server.log"; exit 1)

# Stop the local server
stop-server:
    -pkill -f "beep-server {{server_config}}"

# End-to-end tests on Boat VMs: `just e2e` runs every scenario, `just e2e
# sync` one. It makes the VMs, runs, and deletes them (see tests/e2e/README.md).
e2e scenario="all" action="run":
    tests/e2e/e2e.sh {{scenario}} {{action}}

# Problem reports in R2, newest first: just reports
reports:
    uv run -q --with boto3 python -I scripts/reports.py --config {{server_config}} list

# Download and unzip a report to target/reports/<id>: just report R-ABC...
report id:
    uv run -q --with boto3 python -I scripts/reports.py --config {{server_config}} get {{id}}

# Delete a solved report's zip and manifest: just report-solved R-ABC...
report-solved id:
    uv run -q --with boto3 python -I scripts/reports.py --config {{server_config}} solved {{id}}

# Tail server logs
logs:
    tail -f .dev/server.log

# Tail device logcat for Beep
logcat:
    adb logcat -s beep,BeepApplication,MainActivity --format=brief
