#!/usr/bin/env bash
set -euo pipefail

: "${CARGO_TARGET_DIR:?Set CARGO_TARGET_DIR}"
: "${VERSION:?Set VERSION}"
: "${APPLE_API_KEY_ID:?Set APPLE_API_KEY_ID}"
: "${APPLE_API_ISSUER_ID:?Set APPLE_API_ISSUER_ID}"
: "${RUNNER_TEMP:?Run this script through the macOS release workflow}"

app="$CARGO_TARGET_DIR/release/bundle/macos/Beep.app"
arch=$(uname -m)
output="$PWD/target/macos-release"
mkdir -p "$output"
dmg="$output/Beep-$VERSION-$arch.dmg"

bash scripts/package-macos-dmg.sh "$app" "$dmg"
codesign --sign 'Developer ID Application' --timestamp "$dmg"

result="$output/notary-$arch.json"
xcrun notarytool submit "$dmg" --key "$RUNNER_TEMP/notary-key.p8" \
  --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID" \
  --wait --timeout 30m --output-format json > "$result"
if ! jq -e '.status == "Accepted"' "$result" >/dev/null; then
  echo 'DMG notarization was not accepted.' >&2
  exit 1
fi
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
codesign --verify --strict "$dmg"
spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"

mount="$RUNNER_TEMP/beep-dmg"
hdiutil attach -nobrowse -readonly -mountpoint "$mount" "$dmg"
trap 'hdiutil detach "$mount" -quiet' EXIT
codesign --verify --deep --strict "$mount/Beep.app"
xcrun stapler validate "$mount/Beep.app"
spctl --assess --type execute --verbose=2 "$mount/Beep.app"

cd "$output"
shasum -a 256 "${dmg##*/}" > "${dmg##*/}.sha256"
