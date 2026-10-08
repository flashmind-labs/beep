#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
ndk_dir="${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to an installed Android NDK}"
case "$(uname -s)" in
    Darwin) ndk_host=darwin-x86_64 ;;
    Linux) ndk_host=linux-x86_64 ;;
    *) echo "Unsupported build host" >&2; exit 1 ;;
esac
bin="$ndk_dir/toolchains/llvm/prebuilt/$ndk_host/bin"
export PATH="$HOME/.cargo/bin:$PATH"
# Each line is the Rust target, the NDK compiler prefix and the Android ABI.
for spec in \
    "aarch64-linux-android aarch64-linux-android arm64-v8a" \
    "armv7-linux-androideabi armv7a-linux-androideabi armeabi-v7a" \
    "x86_64-linux-android x86_64-linux-android x86_64" \
    "i686-linux-android i686-linux-android x86"
do
    set -- $spec
    target=$1 abi=$3
    clang="$bin/${2}30-clang"
    env_target=$(echo "$target" | tr 'a-z-' 'A-Z_')
    export "CARGO_TARGET_${env_target}_LINKER=$clang"
    export "CC_$(echo "$target" | tr '-' '_')=$clang"
    export "AR_$(echo "$target" | tr '-' '_')=$bin/llvm-ar"
    cargo build -q -p beep-android --target "$target" --profile android-release
    mkdir -p "apps/android/app/src/main/jniLibs/$abi"
    cp "target/$target/android-release/libbeep_android.so" "apps/android/app/src/main/jniLibs/$abi/"
done
# Kotlin bindings come from the metadata in the built library, which is the same for every ABI.
rm -rf apps/android/app/build/generated/uniffi
cargo run -q -p beep-android --features bindgen --bin uniffi-bindgen -- generate \
    --library target/aarch64-linux-android/android-release/libbeep_android.so \
    --language kotlin --no-format --out-dir apps/android/app/build/generated/uniffi
