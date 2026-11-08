#!/usr/bin/env bash
# Builds (release mode), installs and launches the phone app (the desktop app built for iOS)
# on a device or simulator by name, or with --ipa builds an .ipa to install
# later. Without a name, Tauri picks the connected
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

# Xcode runs cargo with its output hidden. Building the same library here
# first shows the progress, and Xcode's run then finds it up to date.
npm run build
target=aarch64-apple-ios
for arg in "$@"; do [[ $arg == *Simulator* || $arg == *iPhone\ 1[0-9]* && $arg != *"'"* ]] || true; done
cargo build --manifest-path src-tauri/Cargo.toml --package beep-desktop --lib \
    --release --target "$target" --features tauri/custom-protocol
if [[ ${1:-} == --ipa ]]; then
    # Signed for development: installs on the team's registered devices.
    npm run tauri -- ios build -v --export-method debugging
    ls -t src-tauri/gen/apple/build/arm64/*.ipa | head -1
else
    npm run tauri -- ios run -v --release --no-watch ${1:+"$1"}
fi
