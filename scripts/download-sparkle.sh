#!/usr/bin/env bash
# Download the pinned Sparkle framework and signing tools.
set -euo pipefail

version=2.9.6
sha256=52bf9e88cdd972fc0c81501377a880e90d47031bd8ca5462488f843e2609e192
root="$(cd "$(dirname "$0")/.." && pwd)"
destination="$root/apps/desktop/src-tauri/sparkle"
marker="$destination/sparkle-bin/.archive-sha256"

if [[ -d "$destination/Sparkle.framework" && -x "$destination/sparkle-bin/sign_update" && -f "$marker" ]]; then
    if [[ "$(cat "$marker")" == "$sha256" ]]; then exit 0; fi
fi

if [[ -d "$destination/Sparkle.framework" || -d "$destination/sparkle-bin" ]]; then
    echo 'Remove the old native updater installation before downloading another version.' >&2
    exit 1
fi

mkdir -p "$destination"
extract="$(mktemp -d "$destination/.download.XXXXXX")"
trap 'rm -rf "$extract"' EXIT
curl --fail --silent --show-error --location --retry 3 \
    "https://github.com/sparkle-project/Sparkle/releases/download/$version/Sparkle-$version.tar.xz" \
    -o "$extract/sparkle.tar.xz"
printf '%s  %s\n' "$sha256" "$extract/sparkle.tar.xz" | shasum -a 256 -c -
tar -xf "$extract/sparkle.tar.xz" -C "$extract"
test -d "$extract/Sparkle.framework"
test -x "$extract/bin/sign_update"
mv "$extract/Sparkle.framework" "$destination/Sparkle.framework"
mv "$extract/bin" "$destination/sparkle-bin"
printf '%s\n' "$sha256" > "$marker"
