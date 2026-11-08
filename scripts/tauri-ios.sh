#!/usr/bin/env bash
# Builds (release mode), installs and launches the phone app (the desktop app built for iOS)
# on a device or simulator by name. Without a name, Tauri picks the connected
# iPhone or asks.
set -euo pipefail

cd "$(dirname "$0")/../apps/desktop"

# Sign with the local development certificate's team unless one was supplied.
team=${APPLE_DEVELOPMENT_TEAM:-${IOS_TEAM_ID:-}}
if [[ -z "$team" ]]; then
    team=$(security find-certificate -c 'Apple Development' -p |
        openssl x509 -noout -subject -nameopt multiline |
        sed -n 's/^ *organizationalUnitName *= *//p')
fi
if ! [[ "$team" =~ ^[A-Z0-9]{10}$ ]]; then
    echo 'Set IOS_TEAM_ID to your Apple development team ID.' >&2
    exit 1
fi
export APPLE_DEVELOPMENT_TEAM=$team

npm ci --silent
bash ../../scripts/tauri-init.sh ios
npm run tauri -- ios run --release --no-watch ${1:+"$1"}
