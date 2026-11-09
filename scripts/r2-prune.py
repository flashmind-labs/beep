#!/usr/bin/env python3
"""Delete earlier desktop versions from the R2 release folder.

    r2-prune.py <platform> <version>

Removes every object under beep/<platform>/ whose name carries a version other
than <version>. Unversioned files (latest.json, Beep-latest.dmg) stay. Reads
ACCOUNT, CF_ACCESS_KEY_ID, CF_SECRET_ACCESS_KEY and CF_BUCKET from the
environment. Needs only Python and curl.
"""

import os
import re
import subprocess
import sys
import urllib.parse
import xml.etree.ElementTree as ET

EMPTY_SHA256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
VERSIONED = re.compile(r"^Beep[-_](\d+\.\d+\.\d+)[-_.]")
NS = {"s3": "http://s3.amazonaws.com/doc/2006-03-01/"}


def call(method: str, url: str) -> bytes:
    return subprocess.run(
        ["curl", "-sSf", "--retry", "3", "-X", method, "--aws-sigv4", "aws:amz:auto:s3",
         "--user", f"{os.environ['CF_ACCESS_KEY_ID']}:{os.environ['CF_SECRET_ACCESS_KEY']}",
         "-H", f"x-amz-content-sha256: {EMPTY_SHA256}", url],
        capture_output=True, check=True,
    ).stdout


def keys(bucket_url: str, prefix: str):
    token = None
    while True:
        query = {"list-type": "2", "prefix": prefix}
        if token:
            query["continuation-token"] = token
        root = ET.fromstring(call("GET", f"{bucket_url}?{urllib.parse.urlencode(query)}"))
        yield from (k.text for k in root.findall("s3:Contents/s3:Key", NS))
        token = root.findtext("s3:NextContinuationToken", namespaces=NS)
        if root.findtext("s3:IsTruncated", namespaces=NS) != "true" or not token:
            return


def main(platform: str, version: str) -> None:
    bucket_url = f"https://{os.environ['ACCOUNT']}.r2.cloudflarestorage.com/{os.environ['CF_BUCKET']}"
    prefix = f"beep/{platform}/"
    for key in list(keys(bucket_url, prefix)):
        match = VERSIONED.match(key[len(prefix):])
        if match and match[1] != version:
            call("DELETE", f"{bucket_url}/{urllib.parse.quote(key)}")
            print(f"Deleted {key}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
