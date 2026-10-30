#!/bin/sh
# Sets up and starts Beep on this Docker host, then prints the QR code that
# links your phone. Safe to run again: it keeps what .env already holds.
#
#   ./setup.sh            asks for the API keys it cannot generate
#   ./setup.sh --admin    an admin instance: no cap on outgoing traffic
#
# Without a terminal, or with BEEP_SETUP_NONINTERACTIVE=1, it asks nothing.
# Any setting in .env.example given in the environment is written to .env
# (BEEP_PROVIDER_KEY=sk-or-v1-... ./setup.sh), and BEEP_ADMIN=1 is --admin.
set -eu
cd "$(dirname "$0")"

admin=${BEEP_ADMIN:-0}
for arg in "$@"; do
    case "$arg" in
        --admin) admin=1 ;;
        -h | --help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option $arg; see ./setup.sh --help" >&2; exit 1 ;;
    esac
done

interactive=0
if [ -t 0 ] && [ "${BEEP_SETUP_NONINTERACTIVE:-0}" != 1 ]; then
    interactive=1
fi

docker compose version > /dev/null 2>&1 \
    || { echo "Docker with Compose is needed: https://docs.docker.com/get-docker/" >&2; exit 1; }

# The value of KEY in .env, empty when unset.
get() {
    awk -v key="$1" 'index($0, key "=") == 1 { value = substr($0, length(key) + 2) } END { print value }' .env
}

# Sets KEY to VALUE in .env, keeping every other line.
put() {
    KEY=$1 VALUE=$2 awk '
        BEGIN { key = ENVIRON["KEY"]; value = ENVIRON["VALUE"] }
        index($0, key "=") == 1 { print key "=" value; done = 1; next }
        { print }
        END { if (!done) print key "=" value }
    ' .env > .env.partial
    mv .env.partial .env
}

secret() {
    LC_ALL=C tr -dc 'a-z0-9' < /dev/urandom | head -c 32
}

# Asks for KEY when it is empty and someone is there to answer.
ask() {
    [ "$interactive" = 1 ] && [ -z "$(get "$1")" ] || return 0
    printf '%s (Enter to skip): ' "$2"
    stty -echo 2> /dev/null || true
    read -r answer || answer=
    stty echo 2> /dev/null || true
    echo
    if [ -n "$answer" ]; then put "$1" "$answer"; fi
}

umask 077
[ -f .env ] || cp .env.example .env

# Settings given in the environment win over .env.
for key in $(sed -n 's/^\([A-Z][A-Z0-9_]*\)=.*/\1/p' .env.example); do
    if value=$(printenv "$key"); then put "$key" "$value"; fi
done

[ -n "$(get POSTGRES_PASSWORD)" ] || put POSTGRES_PASSWORD "$(secret)"
if [ "$admin" = 1 ]; then
    put BEEP_EGRESS_LIMIT unlimited
elif [ -z "$(get BEEP_EGRESS_LIMIT)" ]; then
    put BEEP_EGRESS_LIMIT 1M
fi

ask BEEP_PROVIDER_KEY "OpenRouter key, needed to answer (https://openrouter.ai/keys)"
ask BEEP_FIRECRAWL_KEY "Firecrawl key, for web search"
ask BEEP_COMPOSIO_KEY "Composio key, for Gmail, Calendar and other apps"

if [ -n "$(get BEEP_SERVER_IMAGE)" ] && [ -n "$(get BEEP_NODE_IMAGE)" ]; then
    docker compose pull
else
    docker compose build
fi

if [ -z "$(get BEEP_PROVIDER_KEY)" ]; then
    # The phone can link now; the server starts once it has a model key.
    docker compose run --rm --no-deps server link
    echo "Add BEEP_PROVIDER_KEY to $(pwd)/.env, then run ./setup.sh again to start Beep."
    exit 0
fi

docker compose up -d
# The server makes its key on first start; the link code needs it.
tries=0
until docker compose exec -T server test -s /data/iroh-key 2> /dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -gt 60 ]; then
        echo "The server did not start; see: docker compose logs server" >&2
        exit 1
    fi
    sleep 1
done
docker compose exec -T -u beep server beep-server link /data/config.toml
