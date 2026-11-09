#!/usr/bin/env bash
set -euo pipefail

log=$(mktemp)
trap 'rm -f "$log"' EXIT

for attempt in 1 2 3; do
    if tauri build --ci --bundles deb,appimage \
        --config '{"bundle":{"createUpdaterArtifacts":true}}' \
        -- --locked 2>&1 | tee "$log"; then
        exit 0
    fi

    if [[ $attempt == 3 ]] || ! grep -Eq 'failed to bundle project:.*http status: 5[0-9]{2}' "$log"; then
        exit 1
    fi

    echo "AppImage download returned a server error; retrying ($attempt/3)." >&2
    sleep "$((attempt * 5))"
done
