#!/usr/bin/env bash
# Forgejo release publishing for the tag in GITHUB_REF_NAME.
# Reads TOKEN and API (the repository's /api/v1 URL) from the environment.
#
#   forgejo-release.sh create <name> <notes-file> <file>...
#       Creates the release and attaches the files.
#   forgejo-release.sh attach <file>...
#       Attaches the files to the tag's existing release.
set -euo pipefail

upload() {
  local release=$1 file name
  shift
  for file in "$@"; do
    name=${file##*/}
    curl -sSf -X POST -H "Authorization: token $TOKEN" \
      -F "attachment=@$file" "$API/releases/$release/assets?name=$name" > /dev/null
  done
}

case "$1" in
  create)
    name=$2
    notes=$3
    shift 3
    release=$(jq -n --arg tag "$GITHUB_REF_NAME" --arg name "$name" \
      --rawfile notes "$notes" \
      '{tag_name: $tag, name: $name, body: $notes}' \
      | curl -sSf -X POST -H "Authorization: token $TOKEN" \
          -H "Content-Type: application/json" --data @- "$API/releases" \
      | jq -r '.id // empty')
    [[ -n "$release" ]] || { echo "No release id in the reply" >&2; exit 1; }
    upload "$release" "$@"
    echo "Published $GITHUB_SERVER_URL/$GITHUB_REPOSITORY/releases/tag/$GITHUB_REF_NAME"
    ;;
  attach)
    shift
    release=$(curl -sSf -H "Authorization: token $TOKEN" \
      "$API/releases/tags/$GITHUB_REF_NAME" | jq -er .id)
    upload "$release" "$@"
    ;;
  *)
    echo "usage: forgejo-release.sh create <name> <notes-file> <file>... | attach <file>..." >&2
    exit 2
    ;;
esac
