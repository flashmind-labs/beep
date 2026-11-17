#!/usr/bin/env bash
# Cross-build a portable Windows executable. Native installers use the PowerShell script.
set -euo pipefail
cd "$(dirname "$0")/../apps/desktop"

if [[ -z "${RUSTC_WRAPPER:-}" ]] && command -v sccache >/dev/null; then
    export RUSTC_WRAPPER=sccache
fi

npm ci --silent
npm exec --yes --package '@tauri-apps/cli@2' -- tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle --ci
