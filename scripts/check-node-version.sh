#!/usr/bin/env bash
# Fails unless the tag in GITHUB_REF_NAME matches crates/node/Cargo.toml, then
# exports VERSION to later steps through GITHUB_ENV.
set -euo pipefail

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/node/Cargo.toml | head -1)
if [[ "node-v$version" != "$GITHUB_REF_NAME" ]]; then
  echo "The tag $GITHUB_REF_NAME does not match beep-node's version $version." >&2
  exit 1
fi
echo "VERSION=$version" >> "$GITHUB_ENV"
