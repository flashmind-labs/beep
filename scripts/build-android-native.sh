#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
ndk_dir="${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to an installed Android NDK}"
case "$(uname -s)" in
    Darwin) ndk_host=darwin-x86_64 ;;
    Linux) ndk_host=linux-x86_64 ;;
    *) echo "Unsupported build host" >&2; exit 1 ;;
esac
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ndk_dir/toolchains/llvm/prebuilt/$ndk_host/bin/aarch64-linux-android30-clang"
export CC_aarch64_linux_android="$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"
export AR_aarch64_linux_android="$ndk_dir/toolchains/llvm/prebuilt/$ndk_host/bin/llvm-ar"
export PATH="$HOME/.cargo/bin:$PATH"
cargo build -q -p beep-android --target aarch64-linux-android --release
mkdir -p android/app/src/main/jniLibs/arm64-v8a
cp target/aarch64-linux-android/release/libbeep_android.so android/app/src/main/jniLibs/arm64-v8a/
# Kotlin bindings come from the metadata in the built library.
rm -rf android/app/build/generated/uniffi
cargo run -q -p beep-android --features bindgen --bin uniffi-bindgen -- generate \
    --library target/aarch64-linux-android/release/libbeep_android.so \
    --language kotlin --no-format --out-dir android/app/build/generated/uniffi
