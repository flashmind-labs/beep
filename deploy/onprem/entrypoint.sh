#!/bin/sh
# Starts the server of the deploy/onprem bundle. It writes /data/config.toml
# from the BEEP_* settings in .env on every start (unless BEEP_CONFIG names a
# config of its own), copies key files from /local, caps outgoing traffic
# (see egress.sh), then runs the server as `beep` with no capabilities. Runs
# as root only for that.
#
# `link` as the command prints the link code instead, without serving.
set -eu
mode=${1:-serve}
umask 077

# A TOML string, quoted.
quoted() {
    printf '"%s"' "$(printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"
}

# `key = "value"` when the value is set.
text() {
    if [ -n "$2" ]; then printf '%s = %s\n' "$1" "$(quoted "$2")"; fi
}

# `key = true|false`, defaulting to true.
flag() {
    value=${2:-true}
    case "$value" in
        true | false) printf '%s = %s\n' "$1" "$value" ;;
        *) echo "$3 must be true or false, not $value" >&2; exit 1 ;;
    esac
}

# A key file from /local, copied where the server reads it.
local_file() {
    if [ -z "$1" ]; then return; fi
    if [ ! -f "/local/$1" ]; then
        echo "$2 names $1, which is not in deploy/onprem/local" >&2
        exit 1
    fi
    mkdir -p /data/local
    cp "/local/$1" "/data/local/$1"
    printf '/data/local/%s' "$1"
}

render() {
    if [ "$mode" != link ]; then
        : "${BEEP_PROVIDER_KEY:?Set BEEP_PROVIDER_KEY in .env}"
    fi
    text secret_key /data/iroh-key
    text log_dir /data/logs
    text model "${BEEP_MODEL:-openrouter:z-ai/glm-5.3-flash}"
    text vision_model "${BEEP_VISION_MODEL:-}"
    text reasoning "${BEEP_REASONING:-}"
    text provider_key "${BEEP_PROVIDER_KEY:-}"
    text firecrawl_key "${BEEP_FIRECRAWL_KEY:-}"
    text openai_key "${BEEP_OPENAI_KEY:-}"
    text mistral_key "${BEEP_MISTRAL_KEY:-}"
    text composio_key "${BEEP_COMPOSIO_KEY:-}"
    text composio_callback_url "${BEEP_COMPOSIO_CALLBACK_URL:-}"
    fcm=$(local_file "${BEEP_FCM_FILE:-}" BEEP_FCM_FILE)
    text fcm_credentials "$fcm"
    # Top-level keys and tables of your own, such as [agents.general].
    if [ -f /local/extra.toml ]; then
        echo
        cat /local/extra.toml
        echo
    fi
    echo '[instance]'
    text owner "${BEEP_OWNER:-}"
    text name "${BEEP_NAME:-Beep}"
    flag events "${BEEP_EVENTS:-}" BEEP_EVENTS
    flag background "${BEEP_BACKGROUND:-}" BEEP_BACKGROUND
    flag nodes "${BEEP_NODES:-}" BEEP_NODES
    flag agents "${BEEP_AGENTS:-}" BEEP_AGENTS
    text local_node /link
    if [ -n "${BEEP_APNS_FILE:-}" ]; then
        apns=$(local_file "$BEEP_APNS_FILE" BEEP_APNS_FILE)
        echo '[apns]'
        text key_file "$apns"
        text key_id "${BEEP_APNS_KEY_ID:?Set BEEP_APNS_KEY_ID with BEEP_APNS_FILE}"
        text team_id "${BEEP_APNS_TEAM_ID:?Set BEEP_APNS_TEAM_ID with BEEP_APNS_FILE}"
        text topic "${BEEP_APNS_TOPIC:?Set BEEP_APNS_TOPIC with BEEP_APNS_FILE}"
    fi
}

config=${BEEP_CONFIG:-}
if [ -z "$config" ]; then
    config=/data/config.toml
    render > "$config.partial"
    mv "$config.partial" "$config"
fi

if [ "$mode" = link ]; then
    set -- link "$config"
else
    # Refuses to go on when the cap cannot be applied.
    beep-egress
    set -- "$config"
fi

if [ "$(id -u)" = 0 ]; then
    # The folder the bundled node shares; each side writes its own file.
    mkdir -p /link
    chmod 1777 /link
    chown -R beep:beep /data
    exec setpriv --reuid beep --regid beep --init-groups \
        --inh-caps=-all --bounding-set=-all \
        env HOME=/home/beep beep-server "$@"
fi
exec beep-server "$@"
