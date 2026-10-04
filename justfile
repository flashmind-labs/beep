set dotenv-load

server_config := ".dev/android-server.toml"
apk := "android/app/build/outputs/apk/debug/app-debug.apk"
# The Docker host the server deploys to, as ssh://host. Set it in .env.
remote_docker := env("BEEP_DOCKER_HOST", "")
# The image Forgejo CI publishes, as registry/user/beep-server. Set it in .env.
server_image := env("BEEP_IMAGE", "")

# Build and deploy everything: server on the remote host + Android APK
deploy: deploy-server bake-profile build-android install-apk launch-app

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

# Server only: redeploy the CI-built image on the remote host
server: deploy-server

# Stop the local server
stop-server:
    -pkill -f "beep-server {{server_config}}"

# Run the image CI built for HEAD on the remote Docker host, with the local
# config and iroh key. The Forgejo runner builds on that host, so the image
# shows up in its daemon: push first, and this waits for it. Stops the local
# server first: both would answer as the same node.
deploy-server:
    #!/usr/bin/env bash
    set -euo pipefail
    test -n "{{remote_docker}}" || { echo "Set BEEP_DOCKER_HOST in .env"; exit 1; }
    test -n "{{server_image}}" || { echo "Set BEEP_IMAGE in .env"; exit 1; }
    export BEEP_TAG=$(git rev-parse HEAD)
    export DOCKER_HOST={{remote_docker}}
    echo "Waiting for {{server_image}}:$BEEP_TAG"
    until docker image inspect "{{server_image}}:$BEEP_TAG" > /dev/null 2>&1; do
        (( SECONDS < 1800 )) || { echo "No image for $BEEP_TAG after 30 minutes"; exit 1; }
        sleep 5
    done
    just stop-server
    just push-server-config
    docker compose up -d --force-recreate
    docker compose ps

# Copy the dev config and iroh key into the remote data volume
push-server-config:
    #!/usr/bin/env bash
    set -euo pipefail
    export DOCKER_HOST={{remote_docker}}
    export BEEP_TAG=${BEEP_TAG:-latest}
    docker volume create beep_data > /dev/null
    write() {
        docker run --rm -i --user beep --entrypoint sh -v beep_data:/data {{server_image}}:$BEEP_TAG \
            -c "umask 077 && cat > /data/$1"
    }
    sed -e 's|^secret_key = .*|secret_key = "/data/iroh-key"|' \
        -e 's|^log_dir = .*|log_dir = "/data/logs"|' -e '/^sandbox_dir = /d' {{server_config}} \
        | { cat; echo 'sandbox_dir = "/data/sandboxes"'; } | write config.toml
    write iroh-key < .dev/iroh-key

# Load the AppArmor profile that lets the server's sandbox use user namespaces
install-apparmor:
    @test -n "{{remote_docker}}" || (echo "Set BEEP_DOCKER_HOST in .env"; exit 1)
    ssh {{trim_start_match(remote_docker, "ssh://")}} \
        'sudo tee /etc/apparmor.d/beep-sandbox > /dev/null && sudo apparmor_parser -r /etc/apparmor.d/beep-sandbox' \
        < deploy/beep-sandbox.apparmor

# Tail the remote server's logs
remote-logs:
    DOCKER_HOST={{remote_docker}} docker compose logs -f

# Android only: rebuild, install, launch
android: bake-profile build-android install-apk launch-app

# Start a local Postgres with pgvector for the server and the tests
test-db:
    #!/usr/bin/env bash
    docker start beep-pg 2>/dev/null || docker run -d --name beep-pg -e POSTGRES_USER=beep -e POSTGRES_PASSWORD=beep -e POSTGRES_DB=beep -p 127.0.0.1:55432:5432 pgvector/pgvector:pg17
    until docker exec beep-pg pg_isready -U beep >/dev/null 2>&1; do sleep 1; done
    echo "export BEEP_TEST_DATABASE_URL=postgres://beep:beep@127.0.0.1:55432/beep"

# Run the server tests against the local Postgres
test: test-db
    BEEP_TEST_DATABASE_URL=postgres://beep:beep@127.0.0.1:55432/beep cargo test -p beep-server

# Tail server logs
logs:
    tail -f .dev/server.log

# Tail device logcat for Beep
logcat:
    adb logcat -s beep,BeepApplication,MainActivity --format=brief
