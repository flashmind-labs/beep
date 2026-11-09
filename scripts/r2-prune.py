#!/usr/bin/env python3
"""Delete earlier versions from an R2 release folder, or list them.

    r2-prune.py <folder> <version>    (beep/node, flashmind/cli, ...)
    r2-prune.py list

Removes every object under <folder>/ whose version is below <version>:
desktop files carry it in the name (Beep-0.6.2-x86_64.deb), node and CLI releases
in a folder (0.4.2/SHA256SUMS). Unversioned files (latest.json, Beep-latest.dmg,
install.sh) stay, and so do newer versions. Reads
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
VERSIONED = re.compile(r"^(?:Beep[-_](\d+\.\d+\.\d+)[-_.]|(\d+\.\d+\.\d+)/)")
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


def parse(version: str) -> tuple[int, ...]:
    return tuple(int(part) for part in version.split("."))


def prune(bucket_url: str, folder: str, version: str) -> None:
    prefix = f"{folder}/"
    for key in list(keys(bucket_url, prefix)):
        match = VERSIONED.match(key[len(prefix):])
        if match and parse(match[1] or match[2]) < parse(version):
            call("DELETE", f"{bucket_url}/{urllib.parse.quote(key)}")
            print(f"Deleted {key}")


def main(args: list[str]) -> None:
    bucket_url = f"https://{os.environ['ACCOUNT']}.r2.cloudflarestorage.com/{os.environ['CF_BUCKET']}"
    if args == ["list"]:
        for prefix in ("beep/", "flashmind/cli/"):
            for key in keys(bucket_url, prefix):
                print(key)
    else:
        prune(bucket_url, *args)


if __name__ == "__main__":
    main(sys.argv[1:])
