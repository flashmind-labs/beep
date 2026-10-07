#!/usr/bin/env bash
# Packs Beep.app into the drag-to-Applications installer.
set -euo pipefail

if [[ $# != 2 ]]; then
    echo 'Usage: bash scripts/package-macos-dmg.sh <Beep.app> <output.dmg>' >&2
    exit 1
fi
app=$1
dmg=$2
[[ -d "$app/Contents" ]] || { echo "$app is not an app bundle" >&2; exit 1; }
root=$(cd "$(dirname "$0")/../apps/desktop" && pwd)
staging=$(mktemp -d "${TMPDIR:-/tmp}/beep-dmg.XXXXXX")
trap 'rm -rf "$staging"' EXIT
mkdir -p "$(dirname "$dmg")"
rm -f "$dmg"
xcrun swift "$root/installer/Background.swift" "$root" "$staging/background.tiff"
uvx --from dmgbuild==1.6.7 dmgbuild -s "$root/installer/dmg.py" \
    -D app="$app" -D background="$staging/background.tiff" \
    -D icon="$app/Contents/Resources/icon.icns" \
    'Install Beep' "$dmg"
