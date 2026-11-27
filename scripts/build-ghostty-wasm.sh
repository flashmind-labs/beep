#!/usr/bin/env bash
# Builds apps/desktop/ghostty/ghostty-vt.wasm, the terminal engine of the
# app's terminals: libghostty-vt at a pinned Ghostty commit with
# kitty-wasm.patch, which keeps Kitty graphics in the WebAssembly build.
# The steps match Ghostty's own release of the module (release-tip.yml).
#
# Zig and Binaryen are downloaded at pinned versions and checked against
# their published SHA-256 sums, so nothing installed on the machine is used.
# They are kept in target/ghostty-wasm/, or $BEEP_GHOSTTY_WORK when set.
# Needs bash, curl, git, tar, xz and node.
#
# Run it after changing a pin or the patch, and commit the module with its
# checksum file and the generated src/stores/terminals/ghostty-abi.ts.
set -euo pipefail

GHOSTTY_COMMIT=2166b446eece5f402809681105b377d8dd602378
ZIG_VERSION=0.17.1
BINARYEN_VERSION=133

repo=$(cd "$(dirname "$0")/.." && pwd)
out="$repo/apps/desktop/ghostty"
work=${BEEP_GHOSTTY_WORK:-$repo/target/ghostty-wasm}

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64)
        zig_platform=aarch64-macos
        zig_sum=75f1bc7a1e9852f17ec9a0c1895f767c4ec56d46bc8a834920eece1d0359dd8d
        binaryen_platform=arm64-macos
        binaryen_sum=ad66da82ac13f163e424b1643f16c6dfcccc98b5966296b43e52d3cab04f84a8
        ;;
    Darwin-x86_64)
        zig_platform=x86_64-macos
        zig_sum=9af07b20319fbd0c92ee46ad23e81d6a782b1aaea2cfb8dcbff35f1d2c7ddbb7
        binaryen_platform=x86_64-macos
        binaryen_sum=13a9b90be775c6389ce3d1f879cb8627bea56708ba8c122983941d53a8199b95
        ;;
    Linux-x86_64)
        zig_platform=x86_64-linux
        zig_sum=3581863e3cbcfd0fce0efc682ce7e9e93da96668a977791d356a26d2a7e9b17a
        binaryen_platform=x86_64-linux
        binaryen_sum=2dc9c7813f5375db93d96ead4b78222fcc3e2677bbb832297af4797782a37489
        ;;
    Linux-aarch64)
        zig_platform=aarch64-linux
        zig_sum=a1546d5646eb184437918d6635ca6b762a0e889eac09cef720ee7a1ed3fe7795
        binaryen_platform=aarch64-linux
        binaryen_sum=89c07ea56faf38d0fbecf36ca8ec0721756716185f265b568e133d427f299bf8
        ;;
    *)
        echo "build-ghostty-wasm: no pinned Zig and Binaryen for $(uname -s) $(uname -m)" >&2
        exit 1
        ;;
esac

sha256() {
    if command -v sha256sum > /dev/null; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

# Downloads $1 to $3 unless it is there, and checks it against $2.
fetch() {
    if [[ ! -f $3 ]]; then
        curl -sSfL --retry 3 -o "$3.part" "$1"
        mv "$3.part" "$3"
    fi

    local got
    got=$(sha256 "$3")
    if [[ $got != "$2" ]]; then
        rm -f "$3"
        echo "build-ghostty-wasm: $1 has SHA-256 $got, expected $2" >&2
        exit 1
    fi
}

# Unpacks archive $1 into directory $2 once.
unpack() {
    if [[ ! -d $2 ]]; then
        mkdir -p "$2.part"
        tar -xf "$1" -C "$2.part" --strip-components 1
        mv "$2.part" "$2"
    fi
}

mkdir -p "$work"

zig_name="zig-$zig_platform-$ZIG_VERSION"
fetch "https://ziglang.org/download/$ZIG_VERSION/$zig_name.tar.xz" "$zig_sum" "$work/$zig_name.tar.xz"
unpack "$work/$zig_name.tar.xz" "$work/$zig_name"

binaryen_name="binaryen-version_$BINARYEN_VERSION-$binaryen_platform"
fetch "https://github.com/WebAssembly/binaryen/releases/download/version_$BINARYEN_VERSION/$binaryen_name.tar.gz" \
    "$binaryen_sum" "$work/$binaryen_name.tar.gz"
unpack "$work/$binaryen_name.tar.gz" "$work/$binaryen_name"

# A fresh checkout each time, so the patch applies to the pinned commit only.
src="$work/ghostty"
rm -rf "$src"
git init -q "$src"
git -C "$src" fetch -q --depth 1 https://github.com/ghostty-org/ghostty.git "$GHOSTTY_COMMIT"
git -C "$src" checkout -q FETCH_HEAD
git -C "$src" apply "$out/kitty-wasm.patch"

(cd "$src" && "$work/$zig_name/zig" build \
    -Demit-lib-vt \
    -Dtarget=wasm32-freestanding \
    -Doptimize=ReleaseFast)

"$work/$binaryen_name/bin/wasm-opt" -O3 --strip-dwarf \
    --enable-simd \
    --enable-bulk-memory \
    --enable-sign-ext \
    --enable-nontrapping-float-to-int \
    --enable-multivalue \
    --enable-reference-types \
    "$src/zig-out/bin/ghostty-vt.wasm" \
    -o "$out/ghostty-vt.wasm"

echo "$(sha256 "$out/ghostty-vt.wasm")  ghostty-vt.wasm" > "$out/ghostty-vt.wasm.sha256"

node "$repo/scripts/ghostty-abi.mjs" "$src/src/terminal/c/abi/wasm32-freestanding.json" \
    "$repo/apps/desktop/src/stores/terminals/ghostty-abi.ts" "$GHOSTTY_COMMIT"

echo "Built $out/ghostty-vt.wasm from Ghostty $GHOSTTY_COMMIT"
