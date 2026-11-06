#!/usr/bin/env bash
# Builds the Rust bridge as a static library for iOS and writes the Swift bindings
# and the baked server node ID into apps/ios/Generated.
#
#   build-rust.sh sim     simulator (aarch64-apple-ios-sim)
#   build-rust.sh device  iPhone (aarch64-apple-ios)
set -euo pipefail
root="$(cd "$(dirname "$0")/../../.." && pwd)"
out="$root/apps/ios/Generated"
case "${1:-sim}" in
  sim) target=aarch64-apple-ios-sim ;;
  device) target=aarch64-apple-ios ;;
  *) echo "usage: $0 sim|device" >&2; exit 1 ;;
esac

cd "$root"
profile=".dev/android-assets/testing-profile.json"
profile_field() {
  [ -f "$profile" ] || return 0
  python3 - "$profile" "$1" <<'PYTHON'
import json
import sys

with open(sys.argv[1]) as source:
    print(json.load(source).get(sys.argv[2], ""))
PYTHON
}
node_id="${BEEP_NODE_ID:-$(profile_field node_id)}"
server_url="${BEEP_SERVER_URL:-$(profile_field server_url)}"
if ! [[ "$node_id" =~ ^[0-9a-fA-F]{64}$ ]]; then
  echo 'Set BEEP_NODE_ID in .env to the server node ID, or run just bake-profile.' >&2
  exit 1
fi
if ! [[ "$server_url" =~ ^https?:// ]]; then
  echo 'Set BEEP_SERVER_URL in .env to the server address, or run just bake-profile.' >&2
  exit 1
fi

# Xcode's environment points cargo at the wrong SDK for host build scripts.
unset SDKROOT IPHONEOS_DEPLOYMENT_TARGET
export IPHONEOS_DEPLOYMENT_TARGET=17.0
cargo rustc -p beep-android --lib --crate-type staticlib --target "$target" --release

mkdir -p "$out"
cargo run -q -p beep-android --features bindgen --bin uniffi-bindgen -- \
  generate --library "target/$target/release/libbeep_android.a" --language swift --out-dir "$out"
# One module map so clang finds both FFI modules on the include path.
cat "$out"/*FFI.modulemap > "$out/module.modulemap"
rm "$out"/*FFI.modulemap

printf 'enum BeepConfig {\n    static let nodeId = "%s"\n    static let serverUrl = "%s"\n}\n' "$node_id" "$server_url" > "$out/BeepConfig.swift"
echo "Built $target, node ID ${node_id:0:12}..."
