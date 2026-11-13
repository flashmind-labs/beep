#!/usr/bin/env bash
# Temporary signing keychain for macOS release jobs.
#
#   keychain.sh import <prefix> <p12-file> <p12-var> <password-var>
#       Creates $RUNNER_TEMP/<prefix>-signing.keychain-db, imports the
#       base64 certificate held in the environment variable <p12-var> (written
#       to $RUNNER_TEMP/<p12-file>, password in <password-var>) and puts the
#       keychain first in the user search list. The previous list is saved in
#       $RUNNER_TEMP/<prefix>-keychains.txt.
#   keychain.sh remove <prefix>
#       Drops the keychain from the search list and deletes it. Files the
#       job wrote are removed by the caller.
case "$1" in
  import)
    set -euo pipefail
    umask 077
    prefix=$2
    p12=$3
    certificate=${!4}
    certificate_password=${!5}
    security list-keychains -d user | sed 's/^ *"//; s/"$//' > "$RUNNER_TEMP/$prefix-keychains.txt"
    keychain="$RUNNER_TEMP/$prefix-signing.keychain-db"
    password=$(openssl rand -hex 32)
    security create-keychain -p "$password" "$keychain"
    security set-keychain-settings -lut 7200 "$keychain"
    security unlock-keychain -p "$password" "$keychain"
    printf '%s' "$certificate" | base64 --decode > "$RUNNER_TEMP/$p12"
    security import "$RUNNER_TEMP/$p12" -k "$keychain" \
      -P "$certificate_password" -T /usr/bin/codesign -T /usr/bin/security
    security set-key-partition-list -S apple-tool:,apple:,codesign: \
      -s -k "$password" "$keychain" >/dev/null
    keychains=("$keychain")
    while IFS= read -r previous; do keychains+=("$previous"); done < "$RUNNER_TEMP/$prefix-keychains.txt"
    security list-keychains -d user -s "${keychains[@]}"
    ;;
  remove)
    set -e
    prefix=$2
    # Drop only this job's keychain. Restoring the list saved at the
    # start would also drop one another job on this Mac added since.
    keychains=()
    while IFS= read -r current; do
      [[ "$current" == */$prefix-signing.keychain-db ]] || keychains+=("$current")
    done < <(security list-keychains -d user | sed 's/^ *"//; s/"$//')
    security list-keychains -d user -s "${keychains[@]}"
    security delete-keychain "$RUNNER_TEMP/$prefix-signing.keychain-db" || true
    ;;
  *)
    echo "usage: keychain.sh import <prefix> <p12-file> <p12-var> <password-var> | remove <prefix>" >&2
    exit 2
    ;;
esac
