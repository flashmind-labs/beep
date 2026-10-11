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

profile=".dev/android-assets/testing-profile.json"
node_id="${BEEP_NODE_ID:-}"
if [ -z "$node_id" ] && [ -f "$profile" ]; then
  node_id=$(sed -n 's/.*"node_id":"\([0-9a-f]*\)".*/\1/p' "$profile")
fi
printf 'enum BeepConfig {\n    static let nodeId = "%s"\n}\n' "$node_id" > "$out/BeepConfig.swift"
echo "Built $target, node ID ${node_id:0:12}..."
