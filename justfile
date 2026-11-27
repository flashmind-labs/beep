set dotenv-load

export NO_COLOR := "1"
export CARGO_TERM_COLOR := "never"

server_config := ".dev/android-server.toml"

# Build the server
build-server:
    cargo build --package beep-server

# Run the desktop app from source: an unsigned debug build with the installed
# Beep's data and mesh, so quit Beep first. `just run apart` starts Beep Dev
# instead, with its own data and an unpaired key, next to an installed Beep.
# The window only loads tauri://localhost, so dev serves the built bundle
# instead of a dev server; press Cmd+R after a frontend change.
run mode="shared":
    cd apps/desktop && npm install --silent && npm run build
    cd apps/desktop/src-tauri && cargo tauri dev --runner ../../../scripts/cargo-plain.sh --no-dev-server {{ if mode == "apart" { "--config tauri.dev.conf.json" } else { "" } }}

# Run the desktop as an optimized release build that keeps its symbols and
# line numbers, so `sample` and Instruments name functions. Same modes as `run`.
run-release mode="shared":
    cd apps/desktop && npm install --silent && npm run build
    cd apps/desktop/src-tauri && CARGO_PROFILE_RELEASE_STRIP=false CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo tauri dev --runner ../../../scripts/cargo-plain.sh --release --no-dev-server {{ if mode == "apart" { "--config tauri.dev.conf.json" } else { "" } }}

# Build the desktop DMG.
desktop-build:
    cd apps/desktop && npm ci --silent
    cd apps/desktop/src-tauri && PATH="/usr/bin:$PATH" cargo tauri build --bundles app
    bash scripts/package-macos-dmg.sh "${CARGO_TARGET_DIR:-target}/release/bundle/macos/Beep.app" target/beep-macos.dmg

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

    app="${CARGO_TARGET_DIR:-target}/release/bundle/macos/Beep.app"
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

# Bump selected apps, commit together and publish main plus their release tags.
# Examples: just patch desktop; just patch desktop beep-node android ios; just patch all.
# Add --dry-run to fetch and check versions without creating a release.
[positional-arguments]
patch +apps:
    #!/usr/bin/env bash
    set -euo pipefail
    uv run --script scripts/release.py patch "$@"

# Bump selected apps to the next 0.x minor version and publish them.
# Include patch release notes since the preceding minor, plus pending entries.
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

# Fail on an import cycle or a poll that the base revision does not have
deps base="origin/main":
    #!/usr/bin/env bash
    set -euo pipefail
    dir="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")/.worktrees/deps-base"
    git fetch -q origin
    if [[ -d $dir ]]; then git -C "$dir" checkout -q --detach "{{base}}"; else git worktree add -q --detach "$dir" "{{base}}"; fi
    python3 scripts/depgraph diff "$dir" .
    python3 scripts/depgraph timers . --check --base "$dir"

# Format, lint and test the desktop app
desktop-check:
    cd apps/desktop && npm ci --silent && npm run build && npm run check
    cd apps/desktop/src-tauri && cargo fmt -p beep-desktop --check && cargo clippy -p beep-desktop --all-targets -- -D warnings && cargo test -p beep-desktop
    just desktop-smoke

# Start the debug desktop app under a virtual display with scratch data and check setup finishes
desktop-smoke:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v xvfb-run >/dev/null; then
        echo "desktop-smoke: xvfb-run not found, skipping"
        exit 0
    fi
    cargo build -p beep-desktop
    bin="${CARGO_TARGET_DIR:-target}/debug/beep-desktop"
    mkdir -p "$HOME/.cache/tmp"
    scratch=$(mktemp -d -p "$HOME/.cache/tmp" desktop-smoke.XXXX)
    err="$scratch/stderr.log"
    # The app logs to a file under the state folder, not to stderr.
    logs="$scratch/state/beep/logs"
    show() { cat "$logs"/* 2>/dev/null; }
    pid=
    runner=
    cleanup() {
        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            kill -TERM "$pid" 2>/dev/null || true
            for _ in $(seq 10); do
                kill -0 "$pid" 2>/dev/null || break
                sleep 1
            done
            kill -KILL "$pid" 2>/dev/null || true
        fi
        wait "$runner" 2>/dev/null || true
        rm -rf "$scratch"
    }
    trap cleanup EXIT
    mkdir -p "$scratch/home" "$scratch/data" "$scratch/config" "$scratch/cache" "$scratch/state"
    mkdir -m 700 "$scratch/run"
    # Scratch dirs and a private session bus keep the run off the real data and desktop.
    env \
        HOME="$scratch/home" XDG_DATA_HOME="$scratch/data" XDG_CONFIG_HOME="$scratch/config" \
        XDG_CACHE_HOME="$scratch/cache" XDG_STATE_HOME="$scratch/state" \
        XDG_RUNTIME_DIR="$scratch/run" TMPDIR="$scratch" \
        WEBKIT_DISABLE_COMPOSITING_MODE=1 \
        xvfb-run -a dbus-run-session -- sh -c 'echo $$ > "$1"; exec "$2"' sh "$scratch/app.pid" "$bin" 2>"$err" &
    runner=$!
    until [ -s "$scratch/app.pid" ]; do
        kill -0 "$runner" 2>/dev/null || break
        sleep 1
    done
    pid=$(cat "$scratch/app.pid" 2>/dev/null || true)
    for _ in $(seq 60); do
        if show | grep -q 'startup: setup done'; then
            echo "desktop-smoke: setup done"
            exit 0
        fi
        if ! kill -0 "$runner" 2>/dev/null; then
            echo "desktop-smoke: app exited before setup finished" >&2
            { tail -n 20 "$err"; show | tail -n 40; } >&2
            exit 1
        fi
        sleep 1
    done
    echo "desktop-smoke: timed out waiting for setup" >&2
    { tail -n 20 "$err"; show | tail -n 40; } >&2
    exit 1

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

# Build the phone app's release APK for arm64, signed with the SDK's debug key for adb installs
android-build:
    #!/usr/bin/env bash
    set -euo pipefail
    bash scripts/tauri-init.sh android
    cd apps/desktop
    npm ci --silent
    npm run tauri -- android build --apk --target aarch64
    # Tauri leaves release APKs unsigned; adb installs need a signature, so use the SDK's debug key.
    store=~/.android/debug.keystore
    [ -f "$store" ] || keytool -genkeypair -keystore "$store" -alias androiddebugkey -storepass android -keypass android \
        -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Android Debug" > /dev/null
    sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:?set ANDROID_HOME}}
    apksigner=$(ls "$sdk"/build-tools/*/apksigner | sort -V | tail -1)
    out=src-tauri/gen/android/app/build/outputs/apk
    apk=$(find "$out" -name '*release-unsigned.apk' | head -1)
    "$apksigner" sign --ks "$store" --ks-key-alias androiddebugkey --ks-pass pass:android --key-pass pass:android \
        --out "$out/beep-android.apk" "$apk"
    echo "adb install -r apps/desktop/$out/beep-android.apk"

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
