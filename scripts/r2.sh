#!/usr/bin/env bash
# R2 helpers for release workflows.
#
#   r2.sh account
#       Prints the account id from CF_ENDPOINT (https://<account>.<host>).
#   r2.sh put <file> <key> <content-type>
#       Uploads the file to the bucket with Cache-Control: no-cache. Reads
#       ACCOUNT, CF_ACCESS_KEY_ID, CF_SECRET_ACCESS_KEY and CF_BUCKET.
set -euo pipefail

case "$1" in
  account)
    account=${CF_ENDPOINT#https://}
    echo "${account%%.*}"
    ;;
  put)
    file=$2
    key=$3
    type=$4
    curl -sSf --retry 3 --aws-sigv4 "aws:amz:auto:s3" \
      --user "$CF_ACCESS_KEY_ID:$CF_SECRET_ACCESS_KEY" \
      -H "x-amz-content-sha256: $(sha256sum "$file" | cut -d ' ' -f 1)" \
      -H "Cache-Control: no-cache" -H "Content-Type: $type" \
      -T "$file" "https://$ACCOUNT.r2.cloudflarestorage.com/$CF_BUCKET/$key" > /dev/null
    ;;
  *)
    echo "usage: r2.sh account | put <file> <key> <content-type>" >&2
    exit 2
    ;;
esac
