#!/bin/sh
# Installs beep-node into ~/.local/bin:
#
#   curl -fsSL https://releases.flashmind.com/beep/node/install.sh | sh
#
# BEEP_NODE_VERSION picks a version instead of the latest one.
# BEEP_NODE_BASE replaces the download location, for testing.
set -eu

base=${BEEP_NODE_BASE:-https://releases.flashmind.com/beep/node}
bindir="$HOME/.local/bin"
tmp=""

say() { printf '%s\n' "$*"; }
fail() { printf 'beep-node install: %s\n' "$*" >&2; exit 1; }

cleanup() {
    if [ -n "$tmp" ]; then rm -rf "$tmp"; fi
    rm -f "$bindir/.beep-node.$$"
}

fetch() {
    if command -v curl > /dev/null 2>&1; then
        curl -fsSL --retry 3 -o "$2" "$1"
    elif command -v wget > /dev/null 2>&1; then
        wget -q -O "$2" "$1"
    else
        fail "needs curl or wget"
    fi
}

sha256() {
    if command -v sha256sum > /dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command -v shasum > /dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        fail "needs sha256sum or shasum to check the download"
    fi
}

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Linux)
            case "$arch" in
                x86_64 | amd64) echo x86_64-unknown-linux-musl ;;
                aarch64 | arm64) echo aarch64-unknown-linux-musl ;;
                *) fail "no build for Linux on $arch" ;;
            esac
            ;;
        Darwin)
            # A shell under Rosetta reports x86_64 on an Apple silicon Mac.
            if [ "$arch" = arm64 ] || [ "$(sysctl -n sysctl.proc_translated 2> /dev/null)" = 1 ]; then
                echo aarch64-apple-darwin
            else
                fail "no build for Intel Macs"
            fi
            ;;
        *) fail "no build for $os" ;;
    esac
}

main() {
    target=$(detect_target)
    trap cleanup EXIT
    trap 'exit 1' HUP INT TERM
    tmp=$(mktemp -d)

    version=${BEEP_NODE_VERSION:-}
    if [ -z "$version" ]; then
        fetch "$base/latest.txt" "$tmp/latest.txt" || fail "could not read the latest version from $base"
        version=$(tr -d ' \t\r\n' < "$tmp/latest.txt")
    fi
    version=${version#v}
    case "$version" in
        "" | *[!0-9.]*) fail "bad version '$version'" ;;
    esac

    archive="beep-node-$version-$target.tar.gz"
    say "Downloading beep-node $version for $target"
    fetch "$base/$version/$archive" "$tmp/$archive" || fail "could not download $base/$version/$archive"
    fetch "$base/$version/SHA256SUMS" "$tmp/SHA256SUMS" || fail "could not download $base/$version/SHA256SUMS"

    expected=$(awk -v f="$archive" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS")
    [ -n "$expected" ] || fail "$archive is not listed in SHA256SUMS"
    actual=$(sha256 "$tmp/$archive")
    if [ "$actual" != "$expected" ]; then
        fail "checksum mismatch for $archive (expected $expected, got $actual); nothing was installed"
    fi

    tar -xzf "$tmp/$archive" -C "$tmp" beep-node
    mkdir -p "$bindir"
    # Copy next to the destination, then rename over it, so a running
    # beep-node is never left half written.
    cp "$tmp/beep-node" "$bindir/.beep-node.$$"
    chmod 755 "$bindir/.beep-node.$$"
    mv -f "$bindir/.beep-node.$$" "$bindir/beep-node"
    say "Installed $bindir/beep-node"

    case ":$PATH:" in
        *":$bindir:"*) ;;
        *)
            say ""
            say "$bindir is not on your PATH. Add it with:"
            # Printed for the user to run; $PATH must stay unexpanded.
            # shellcheck disable=SC2016
            say '  echo '\''export PATH="$HOME/.local/bin:$PATH"'\'' >> ~/.profile'
            say "then open a new shell."
            ;;
    esac

    say ""
    say "Next, link this machine to Beep: open Add a machine in Beep and run"
    say "  beep-node setup <token>"
    say "with the token it shows."
}

main "$@"
