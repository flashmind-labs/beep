#!/usr/bin/env bash
# Sign inside out before bundling, preserving the downloader's sandbox.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
framework="$root/apps/desktop/src-tauri/sparkle/Sparkle.framework"
version="$framework/Versions/B"
identity="${APPLE_SIGNING_IDENTITY:-Developer ID Application}"

codesign --force --options runtime --timestamp --sign "$identity" \
    "$version/XPCServices/Installer.xpc"
codesign --force --options runtime --timestamp --preserve-metadata=entitlements \
    --sign "$identity" "$version/XPCServices/Downloader.xpc"
codesign --force --options runtime --timestamp --sign "$identity" "$version/Autoupdate"
codesign --force --options runtime --timestamp --sign "$identity" "$version/Updater.app"
codesign --force --options runtime --timestamp --sign "$identity" "$framework"
codesign --verify --deep --strict "$framework"
