set dotenv-load

server_config := ".dev/android-server.toml"
# The local Postgres `test-db` starts, which the server tests use.
test_db_url := "postgres://beep:beep@127.0.0.1:55432/beep"

# Serve the platform mockups on localhost
mockups port="8000":
    python3 -m http.server {{port}} --bind 127.0.0.1 --directory mockups

# Build the server
build-server:
    cargo build --package beep-server

# Download the pinned native macOS updater framework and signing tools, and
# sign the framework as the app is signed: ad hoc unless APPLE_SIGNING_IDENTITY
# names an identity. macOS refuses to load a framework signed by another team.
desktop-sparkle:
    if [ "$(uname -s)" = Darwin ]; then bash scripts/download-sparkle.sh && APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:--}" bash scripts/sign-sparkle.sh; fi

# Run the macOS desktop app from source
desktop-dev: desktop-sparkle
    cd apps/desktop && npm ci --silent
    cd apps/desktop/src-tauri && cargo tauri dev

# Build the desktop DMG.
desktop-build: desktop-sparkle
    cd apps/desktop && npm ci --silent
    cd apps/desktop/src-tauri && PATH="/usr/bin:$PATH" cargo tauri build --bundles app
    bash scripts/package-macos-dmg.sh "${CARGO_TARGET_DIR:-apps/desktop/src-tauri/target}/release/bundle/macos/Beep.app" target/beep-macos.dmg

# Build the desktop DMG signed with a Developer ID from the keychain.
desktop-build-signed: desktop-sparkle
    #!/usr/bin/env bash
    set -euo pipefail
    export APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-Developer ID Application}"
    if ! security find-identity -v -p codesigning | grep -qF -- "$APPLE_SIGNING_IDENTITY"; then
        echo "no \"$APPLE_SIGNING_IDENTITY\" signing identity in the keychain" >&2
        exit 1
    fi

    bash scripts/sign-sparkle.sh
    (cd apps/desktop && npm ci --silent)
    (cd apps/desktop/src-tauri && PATH="/usr/bin:$PATH" cargo tauri build --bundles app)

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

# Set the desktop app's version, commit it and tag desktop-v<version>.
# Pushing the tag makes the desktop workflow build, sign, notarize and publish it.
desktop-release version:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "{{version}}" =~ ^0\.[0-9]+\.[0-9]+$ ]] || { echo "version must be like 0.2.3: every app stays on 0.x until production is ready" >&2; exit 1; }
    [[ $(git branch --show-current) == main ]] || { echo "release from main" >&2; exit 1; }
    [[ -z $(git status --porcelain) ]] || { echo "commit or stash your changes first" >&2; exit 1; }
    ! git rev-parse -q --verify "refs/tags/desktop-v{{version}}" > /dev/null || { echo "desktop-v{{version}} already exists" >&2; exit 1; }
    python3 scripts/changelog.py freeze {{version}}
    sed -i.bak '1,/^version = /s/^version = ".*"$/version = "{{version}}"/' apps/desktop/src-tauri/Cargo.toml && rm apps/desktop/src-tauri/Cargo.toml.bak
    (cd apps/desktop/src-tauri && cargo update --workspace --offline --quiet)
    git commit --quiet -m "Release Beep desktop {{version}}" changelogs/desktop.md changelogs/unreleased/desktop apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock
    git tag -a "desktop-v{{version}}" -m "Beep desktop {{version}}"
    echo "Tagged desktop-v{{version}}. Publish it with: git push forgejo main desktop-v{{version}}"

# Set the Android version, commit it and tag android-v<version>; the tag uploads to Play's beta track
android-release version:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "{{version}}" =~ ^0\.[0-9]+\.[0-9]+$ ]] || { echo "version must be like 0.2.3: every app stays on 0.x until production is ready" >&2; exit 1; }
    [[ $(git branch --show-current) == main ]] || { echo "release from main" >&2; exit 1; }
    [[ -z $(git status --porcelain) ]] || { echo "commit or stash your changes first" >&2; exit 1; }
    ! git rev-parse -q --verify "refs/tags/android-v{{version}}" > /dev/null || { echo "android-v{{version}} already exists" >&2; exit 1; }
    python3 scripts/changelog.py freeze {{version}} --path changelogs/android.md
    cfg=apps/desktop/src-tauri/tauri.android.conf.json
    sed -i.bak 's/^\( *"version": \)".*"/\1"{{version}}"/' "$cfg" && rm "$cfg.bak"
    # The version may already be set, as in a first release.
    git commit --quiet -m "Release Beep for Android {{version}}" changelogs/android.md changelogs/unreleased/android "$cfg"
    git tag -a "android-v{{version}}" -m "Beep for Android {{version}}"
    echo "Tagged android-v{{version}}. Publish it with: git push origin main android-v{{version}}"

# Set the iOS version, commit it and tag ios-v<version>; the tag uploads to TestFlight
ios-release version:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "{{version}}" =~ ^0\.[0-9]+\.[0-9]+$ ]] || { echo "version must be like 0.2.3: every app stays on 0.x until production is ready" >&2; exit 1; }
    [[ $(git branch --show-current) == main ]] || { echo "release from main" >&2; exit 1; }
    [[ -z $(git status --porcelain) ]] || { echo "commit or stash your changes first" >&2; exit 1; }
    ! git rev-parse -q --verify "refs/tags/ios-v{{version}}" > /dev/null || { echo "ios-v{{version}} already exists" >&2; exit 1; }
    python3 scripts/changelog.py freeze {{version}} --path changelogs/ios.md
    cfg=apps/desktop/src-tauri/tauri.ios.conf.json
    sed -i.bak 's/^\( *"version": \)".*"/\1"{{version}}"/' "$cfg" && rm "$cfg.bak"
    # The version may already be set, as in a first release.
    git commit --quiet -m "Release Beep for iOS {{version}}" changelogs/ios.md changelogs/unreleased/ios "$cfg"
    git tag -a "ios-v{{version}}" -m "Beep for iOS {{version}}"
    echo "Tagged ios-v{{version}}. Publish it with: git push origin main ios-v{{version}}"

# Set beep-node's version, commit it and tag node-v<version>. Pushing the tag
# makes the node workflow build the Linux and macOS binaries and publish them
# with install.sh at https://releases.flashmind.com/beep/node/.
node-release version:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "{{version}}" =~ ^0\.[0-9]+\.[0-9]+$ ]] || { echo "version must be like 0.2.3: every app stays on 0.x until production is ready" >&2; exit 1; }
    [[ $(git branch --show-current) == main ]] || { echo "release from main" >&2; exit 1; }
    [[ -z $(git status --porcelain) ]] || { echo "commit or stash your changes first" >&2; exit 1; }
    ! git rev-parse -q --verify "refs/tags/node-v{{version}}" > /dev/null || { echo "node-v{{version}} already exists" >&2; exit 1; }
    python3 scripts/changelog.py freeze {{version}} --path changelogs/node.md
    sed -i.bak '1,/^version = /s/^version = ".*"$/version = "{{version}}"/' crates/node/Cargo.toml && rm crates/node/Cargo.toml.bak
    cargo update --workspace --offline --quiet
    # The desktop app embeds the node, so its lockfile names the version too.
    # Resolving the lockfile records just that; `cargo update -p beep-node`
    # would also move the node's git dependencies.
    (cd apps/desktop/src-tauri && cargo metadata --offline --format-version 1 > /dev/null)
    git commit --quiet -m "Release beep-node {{version}}" changelogs/node.md changelogs/unreleased/node crates/node/Cargo.toml Cargo.lock apps/desktop/src-tauri/Cargo.lock
    git tag -a "node-v{{version}}" -m "beep-node {{version}}"
    echo "Tagged node-v{{version}}. Publish it with: git push origin main node-v{{version}}"

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

# Deploy the Cloudflare Worker behind https share and invite links. Needs
# CLOUDFLARE_ACCOUNT_ID in .env: the account that holds flashmind.com.
links-deploy:
    test -n "${CLOUDFLARE_ACCOUNT_ID:-}" || { echo "Set CLOUDFLARE_ACCOUNT_ID in .env"; exit 1; }
    cd deploy/links && npx wrangler deploy

# Format, lint and test the desktop app
desktop-check: desktop-sparkle
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

# Start a local Postgres with pgvector for the server and the tests
test-db:
    #!/usr/bin/env bash
    set -euo pipefail
    # Disposable and tuned for speed: data in memory, nothing synced to disk,
    # and connections for every test thread. An older beep-pg is replaced.
    if ! docker inspect -f '{{{{.Config.Cmd}}' beep-pg 2>/dev/null | grep -q fsync=off; then
        docker rm -f beep-pg >/dev/null 2>&1 || true
        docker run -d --name beep-pg -e POSTGRES_USER=beep -e POSTGRES_PASSWORD=beep -e POSTGRES_DB=beep \
            -p 127.0.0.1:55432:5432 --tmpfs /var/lib/postgresql/data --shm-size 1g pgvector/pgvector:pg17 \
            -c fsync=off -c synchronous_commit=off -c full_page_writes=off -c max_connections=1000 >/dev/null
    fi
    docker start beep-pg >/dev/null
    until docker exec beep-pg pg_isready -h 127.0.0.1 -U beep >/dev/null 2>&1; do sleep 1; done
    echo "export BEEP_TEST_DATABASE_URL={{test_db_url}}"

# Run the server tests against the local Postgres
test: test-db
    BEEP_TEST_DATABASE_URL={{test_db_url}} cargo test -p beep-server

# End-to-end tests on Boat VMs: `just e2e` runs every scenario, `just e2e
# sync` one. It makes the VMs, runs, and deletes them (see tests/e2e/README.md).
e2e scenario="all" action="run":
    tests/e2e/e2e.sh {{scenario}} {{action}}

# Print app tools' arguments: `just tool-schema GMAIL_MOVE_TO_TRASH
# FASTMAIL_DELETE_EMAIL`, or `just tool-schema --list outlook` for a toolkit.
tool-schema *args:
    uv run -q --with 'psycopg[binary]' --with httpx python -I scripts/tool-schema.py --config {{server_config}} {{args}}

# Model spend per kind of turn and the costliest user-days: just usage [days]
usage days="14":
    uv run -q --with 'psycopg[binary]' python -I scripts/usage.py --config {{server_config}} {{days}}

# Problem reports in R2, newest first: just reports
reports:
    uv run -q --with boto3 python -I scripts/reports.py --config {{server_config}} list

# Download and unzip a report to target/reports/<id>: just report R-ABC...
report id:
    uv run -q --with boto3 python -I scripts/reports.py --config {{server_config}} get {{id}}

# Delete a solved report's zip and manifest: just report-solved R-ABC...
report-solved id:
    uv run -q --with boto3 python -I scripts/reports.py --config {{server_config}} solved {{id}}

# Run the prompt evals against the real model in the dev config. They cost
# money. `just eval search` runs the cases whose name contains "search".
eval filter="": test-db
    BEEP_TEST_DATABASE_URL={{test_db_url}} BEEP_EVAL="{{filter}}" \
        cargo test -p beep-server --test evals -- --ignored --nocapture

# Follow-up chips for fixed turns, each graded by a judge model: just eval-followups [filter]
eval-followups filter="":
    BEEP_EVAL="{{filter}}" cargo test -p beep-server --test followups -- --ignored --nocapture

# The group leak gate on fixed replies, judged by Jev: just eval-leak [filter]
eval-leak filter="":
    BEEP_EVAL="{{filter}}" cargo test -p beep-server --test leak -- --ignored --nocapture

# The reply gates on fixed replies, judged by Jev: just eval-gates [filter]
eval-gates filter="":
    BEEP_EVAL="{{filter}}" cargo test -p beep-server --test gates -- --ignored --nocapture

# List the OpenRouter models Beep could use, newest first: just models [max-output-price] [filter]
models *args:
    cargo run -q --package beep-server --bin beep-models -- {{args}}

# Tail server logs
logs:
    tail -f .dev/server.log

# Tail device logcat for Beep
logcat:
    adb logcat -s beep,BeepApplication,MainActivity --format=brief
