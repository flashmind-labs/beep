#!/usr/bin/env bash
# The phone app (the desktop app built for iOS), in release mode:
#   tauri-ios.sh --ipa      build an .ipa; `just ios-install` puts it on an iPhone
#   tauri-ios.sh --sim NAME build, install and launch on a simulator
set -euo pipefail

cd "$(dirname "$0")/../apps/desktop"

case ${1:-} in
    --ipa | --sim) ;;
    *) echo 'usage: tauri-ios.sh --ipa | --sim NAME' >&2; exit 1 ;;
esac

# Sign with the local development certificate's team unless one was supplied.
team=${APPLE_DEVELOPMENT_TEAM:-${IOS_TEAM_ID:-}}
if [[ -z "$team" ]]; then
    team=$(security find-certificate -c 'Apple Development' -p |
        openssl x509 -noout -subject -nameopt multiline |
        sed -n 's/^ *organizationalUnitName *= *//p')
fi
if ! [[ "$team" =~ ^[A-Z0-9]{10}$ ]]; then
    echo 'Set IOS_TEAM_ID to your Apple development team ID.' >&2
    exit 1
fi
export APPLE_DEVELOPMENT_TEAM=$team

# On Xcode 27, swift-rs needs llvm-objcopy to export the Swift plugin symbols
# (`_register_plugin` and the like); without it linking fails.
rustup component add llvm-tools

npm ci --silent
bash ../../scripts/tauri-init.sh ios

# Xcode runs cargo itself and shows nothing until it ends. A `cargo` first on
# PATH copies what cargo prints to a file this terminal follows.
real_cargo=$(command -v cargo)
work=$(mktemp -d)
progress=$work/cargo.log
: > "$progress"
cat > "$work/cargo" <<SHIM
#!/usr/bin/env bash
set -o pipefail
{ "$real_cargo" "\$@" 2>&1 1>&3 | tee -a "$progress" >&2; } 3>&1
SHIM
chmod +x "$work/cargo"
export PATH="$work:$PATH"
# Cargo draws its progress bar into the file too, at this terminal's width.
export CARGO_TERM_PROGRESS_WHEN=always CARGO_TERM_PROGRESS_WIDTH=$(tput cols 2>/dev/null || echo 100)
tail -n +1 -f "$progress" &
follower=$!
# Stopped quietly at exit, without the shell's "Terminated" line.
disown "$follower"
trap 'kill "$follower" 2>/dev/null; rm -rf "$work"' EXIT

case ${1:-} in
    --ipa)
        # Signed for development: installs on the team's registered devices.
        npm run tauri -- ios build -v --export-method debugging
        ls -t src-tauri/gen/apple/build/arm64/*.ipa | head -1
        ;;
    --sim)
        # Installed with simctl: `tauri ios run` wants ios-deploy from brew.
        npm run tauri -- ios build -v --target aarch64-sim
        xcrun simctl boot "${2:?simulator name}" 2>/dev/null || true
        open -a Simulator
        xcrun simctl install "$2" src-tauri/gen/apple/build/arm64-sim/Beep.app
        xcrun simctl launch "$2" com.flashmind.beep
        ;;
esac
