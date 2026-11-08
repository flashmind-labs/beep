#!/usr/bin/env bash
# The phone app (the desktop app built for iOS), in release mode:
#   tauri-ios.sh --ipa      build an .ipa; `just ios-install` puts it on an iPhone
#   tauri-ios.sh --sim NAME build, install and launch on a simulator
set -euo pipefail

cd "$(dirname "$0")/../apps/desktop"

case ${1:-} in
    --ipa | --sim) ;;
    *) echo 'usage: tauri-ios.sh --ipa | --sim NAME' >&2; exit 1 ;;
esac

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

target=aarch64-apple-ios
[[ ${1:-} == --sim ]] && target=aarch64-apple-ios-sim

# On Xcode 27, swift-rs needs llvm-objcopy to export the Swift plugin symbols
# (`_register_plugin` and the like); without it linking fails.
rustup component add llvm-tools

npm ci --silent
bash ../../scripts/tauri-init.sh ios

# Xcode runs cargo with its output hidden. Building the dependencies here
# first shows the progress, and Xcode's run then reuses them. Only the static
# library: the cdylib needs Tauri's Swift half, which Xcode links in.
npm run build
cargo rustc --manifest-path src-tauri/Cargo.toml --package beep-desktop --lib \
    --crate-type staticlib --release --target "$target" --features tauri/custom-protocol

case ${1:-} in
    --ipa)
        # Signed for development: installs on the team's registered devices.
        npm run tauri -- ios build -v --export-method debugging
        ls -t src-tauri/gen/apple/build/arm64/*.ipa | head -1
        ;;
    --sim) npm run tauri -- ios run -v --release --no-watch "${2:?simulator name}" ;;
esac
