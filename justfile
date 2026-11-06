set dotenv-load

server_config := ".dev/android-server.toml"
apk := "apps/android/app/build/outputs/apk/debug/app-debug.apk"
# The local Postgres `test-db` starts, which the server tests use.
test_db_url := "postgres://beep:beep@127.0.0.1:55432/beep"

# Build the server
build-server:
    cargo build --package beep-server

# Build the Android debug APK
build-android:
    cd apps/android && ./gradlew assembleDebug

# Run the macOS desktop app from source
desktop-dev:
    cd apps/desktop && npm ci --silent
    cd apps/desktop/src-tauri && cargo tauri dev

# Build the desktop DMG.
desktop-build:
    cd apps/desktop && npm ci --silent
    cd apps/desktop/src-tauri && PATH="/usr/bin:$PATH" cargo tauri build --bundles app
    bash scripts/package-macos-dmg.sh "${CARGO_TARGET_DIR:-apps/desktop/src-tauri/target}/release/bundle/macos/Beep.app" target/beep-macos.dmg

# Tag HEAD server-v<version> and push the tag
server-release version:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "{{version}}" =~ ^0\.[0-9]+\.[0-9]+$ ]] || { echo "version must be like 0.2.3: every app stays on 0.x until production is ready" >&2; exit 1; }
    [[ $(git branch --show-current) == main ]] || { echo "release from main" >&2; exit 1; }
    [[ -z $(git status --porcelain) ]] || { echo "commit or stash your changes first" >&2; exit 1; }
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
    sed -i.bak '1,/^version = /s/^version = ".*"$/version = "{{version}}"/' apps/desktop/src-tauri/Cargo.toml && rm apps/desktop/src-tauri/Cargo.toml.bak
    (cd apps/desktop/src-tauri && cargo update --workspace --offline --quiet)
    git commit --quiet -m "Release Beep for macOS {{version}}" apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock
    git tag -a "desktop-v{{version}}" -m "Beep for macOS {{version}}"
    echo "Tagged desktop-v{{version}}. Publish it with: git push forgejo main desktop-v{{version}}"

# Set the Android app's version, commit it and tag android-v<version>. Pushing
# the tag uploads it to Play's beta track.
android-release version:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "{{version}}" =~ ^0\.[0-9]+\.[0-9]+$ ]] || { echo "version must be like 0.2.3: every app stays on 0.x until production is ready" >&2; exit 1; }
    [[ $(git branch --show-current) == main ]] || { echo "release from main" >&2; exit 1; }
    [[ -z $(git status --porcelain) ]] || { echo "commit or stash your changes first" >&2; exit 1; }
    ! git rev-parse -q --verify "refs/tags/android-v{{version}}" > /dev/null || { echo "android-v{{version}} already exists" >&2; exit 1; }
    sed -i.bak 's/^\( *versionName = \)".*"$/\1"{{version}}"/' apps/android/app/build.gradle.kts && rm apps/android/app/build.gradle.kts.bak
    git commit --quiet -m "Release Beep for Android {{version}}" apps/android/app/build.gradle.kts
    git tag -a "android-v{{version}}" -m "Beep for Android {{version}}"
    echo "Tagged android-v{{version}}. Publish it with: git push origin main android-v{{version}}"

# Set the iOS app's version, commit it and tag ios-v<version>. Pushing the tag
# uploads it to TestFlight.
ios-release version:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "{{version}}" =~ ^0\.[0-9]+\.[0-9]+$ ]] || { echo "version must be like 0.2.3: every app stays on 0.x until production is ready" >&2; exit 1; }
    [[ $(git branch --show-current) == main ]] || { echo "release from main" >&2; exit 1; }
    [[ -z $(git status --porcelain) ]] || { echo "commit or stash your changes first" >&2; exit 1; }
    ! git rev-parse -q --verify "refs/tags/ios-v{{version}}" > /dev/null || { echo "ios-v{{version}} already exists" >&2; exit 1; }
    sed -i.bak 's/^\( *MARKETING_VERSION: \)".*"$/\1"{{version}}"/' apps/ios/project.yml && rm apps/ios/project.yml.bak
    git commit --quiet -m "Release Beep for iOS {{version}}" apps/ios/project.yml
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
    sed -i.bak '1,/^version = /s/^version = ".*"$/version = "{{version}}"/' crates/node/Cargo.toml && rm crates/node/Cargo.toml.bak
    cargo update --workspace --offline --quiet
    # The desktop app embeds the node, so its lockfile names the version too.
    # Resolving the lockfile records just that; `cargo update -p beep-node`
    # would also move the node's git dependencies.
    (cd apps/desktop/src-tauri && cargo metadata --offline --format-version 1 > /dev/null)
    git commit --quiet -m "Release beep-node {{version}}" crates/node/Cargo.toml Cargo.lock apps/desktop/src-tauri/Cargo.lock
    git tag -a "node-v{{version}}" -m "beep-node {{version}}"
    echo "Tagged node-v{{version}}. Publish it with: git push origin main node-v{{version}}"

# Release Android, desktop and beep-node together and push the tags. Without
# versions it bumps each app's patch number: `just release`, or
# `just release 0.2.0 0.4.0 0.1.1 0.2.0` (Android, desktop, node, then iOS).
# The node and iOS are left out until their first `node-release` or
# `ios-release` tag exists.
release android="" desktop="" node="" ios="":
    #!/usr/bin/env bash
    set -euo pipefail
    latest() { git tag --list "$1-v*" --sort=-v:refname | head -1; }
    bump() { local v; v=$(latest "$1"); v=${v#"$1-v"}; [[ -n $v ]] || { echo "no $1 tag to bump" >&2; exit 1; }; echo "${v%.*}.$(( ${v##*.} + 1 ))"; }
    android="{{android}}"; desktop="{{desktop}}"; node="{{node}}"; ios="{{ios}}"
    android=${android:-$(bump android)}; desktop=${desktop:-$(bump desktop)}
    if [[ -z $node && -n $(latest node) ]]; then node=$(bump node); fi
    if [[ -z $ios && -n $(latest ios) ]]; then ios=$(bump ios); fi
    tags=("android-v$android" "desktop-v$desktop")
    if [[ -n $node ]]; then tags+=("node-v$node"); fi
    if [[ -n $ios ]]; then tags+=("ios-v$ios"); fi
    # Checked before tagging anything, so a bad version leaves no tags behind.
    for tag in "${tags[@]}"; do
        [[ ${tag#*-v} =~ ^0\.[0-9]+\.[0-9]+$ ]] || { echo "$tag: version must be like 0.2.3: every app stays on 0.x until production is ready" >&2; exit 1; }
        ! git rev-parse -q --verify "refs/tags/$tag" > /dev/null || { echo "$tag already exists" >&2; exit 1; }
    done
    echo "Releasing Android $android, desktop $desktop${node:+, node $node}${ios:+, iOS $ios}"
    just android-release "$android"
    just desktop-release "$desktop"
    if [[ -n $node ]]; then just node-release "$node"; fi
    if [[ -n $ios ]]; then just ios-release "$ios"; fi
    git push origin main "${tags[@]}"

# Deploy the Cloudflare Worker behind https share and invite links. Needs
# CLOUDFLARE_ACCOUNT_ID in .env: the account that holds flashmind.com.
links-deploy:
    test -n "${CLOUDFLARE_ACCOUNT_ID:-}" || { echo "Set CLOUDFLARE_ACCOUNT_ID in .env"; exit 1; }
    cd deploy/links && npx wrangler deploy

# Format, lint and test the desktop app
desktop-check:
    cd apps/desktop && npm ci --silent && npm run build && npm run check
    cd apps/desktop/src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test

# Build the iOS app for the simulator (Rust bridge, bindings, Xcode project)
ios-build:
    bash apps/ios/scripts/build-rust.sh sim
    cd apps/ios && xcodegen generate --quiet && xcodebuild -project Beep.xcodeproj -scheme Beep -sdk iphonesimulator -destination 'platform=iOS Simulator,name=iPhone 17' -derivedDataPath build -quiet build

# Install and launch the iOS app on the iPhone 17 simulator
ios-sim-run: ios-build
    xcrun simctl boot "iPhone 17" || true
    open -a Simulator
    xcrun simctl install "iPhone 17" apps/ios/build/Build/Products/Debug-iphonesimulator/Beep.app
    xcrun simctl launch "iPhone 17" com.flashmind.beep

# Build, install, and launch on a connected iPhone. Optional device ID or name.
[positional-arguments]
ios-run device="":
    bash apps/ios/scripts/run-device.sh "$1"

# Restart the server (kill old, start new)
restart-server:
    -pkill -f "beep-server {{server_config}}"
    @sleep 1
    bash -c 'target/debug/beep-server {{server_config}} > .dev/server.log 2>&1 & disown'
    @sleep 2
    @pgrep -f "beep-server {{server_config}}" > /dev/null && echo "Server started" || (echo "Server failed to start, check .dev/server.log"; exit 1)

# Write the server's node ID where the Android build picks it up
bake-profile:
    #!/usr/bin/env bash
    node_id=$(sed 's/\x1b\[[0-9;]*m//g' .dev/logs/beep.log | grep -o 'node_id=[0-9a-f]*' | tail -1 | cut -d= -f2)
    if [ -z "$node_id" ]; then echo "No node ID in server log"; exit 1; fi
    mkdir -p .dev/android-assets
    printf '{"node_id":"%s"}\n' "$node_id" > .dev/android-assets/testing-profile.json
    echo "Baked node ID: ${node_id:0:12}..."

# Install APK on connected device
install-apk:
    adb install -r {{apk}}

# Launch the app on device
launch-app:
    adb shell am start -n com.flashmind.beep/.MainActivity

# Stop the local server
stop-server:
    -pkill -f "beep-server {{server_config}}"

# Android only: rebuild, install, launch
android: bake-profile build-android install-apk launch-app

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
