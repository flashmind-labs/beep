#!/usr/bin/env python3
"""Upload an app bundle to a Google Play track.

    play-upload.py <service-account.json> <app.aab> <track> <status>

The status is `draft` until the app has been published once, after that
`completed`. Needs only Python, curl and openssl.

Play keeps one open edit per app, so a second upload running at the same time
(a push to main and a release tag) deletes this one's edit. The upload then
starts over with a new edit.
"""

import base64
import json
import subprocess
import sys
import tempfile
import time

PACKAGE = "com.flashmind.beep"
API = f"https://androidpublisher.googleapis.com/androidpublisher/v3/applications/{PACKAGE}"
UPLOAD = f"https://androidpublisher.googleapis.com/upload/androidpublisher/v3/applications/{PACKAGE}"


def b64(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def access_token(account: dict) -> str:
    now = int(time.time())
    header = b64(json.dumps({"alg": "RS256", "typ": "JWT"}).encode())
    claims = b64(json.dumps({
        "iss": account["client_email"],
        "scope": "https://www.googleapis.com/auth/androidpublisher",
        "aud": account["token_uri"],
        "iat": now,
        "exp": now + 3600,
    }).encode())
    with tempfile.NamedTemporaryFile("w", suffix=".pem") as key:
        key.write(account["private_key"])
        key.flush()
        signature = subprocess.run(
            ["openssl", "dgst", "-sha256", "-sign", key.name],
            input=f"{header}.{claims}".encode(), capture_output=True, check=True,
        ).stdout
    body = (
        "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer"
        f"&assertion={header}.{claims}.{b64(signature)}"
    ).encode()
    return call("POST", account["token_uri"], body, "application/x-www-form-urlencoded")["access_token"]


class EditDeleted(Exception):
    pass


def call(method, url, body=None, content_type="application/json", token=None):
    command = ["curl", "--silent", "--show-error", "--request", method,
               "--header", f"Content-Type: {content_type}",
               "--write-out", "\\n%{http_code}", "--data-binary", "@-", url]
    if token:
        command[1:1] = ["--header", f"Authorization: Bearer {token}"]
    result = subprocess.run(command, input=body or b"", capture_output=True, check=True)
    text, _, code = result.stdout.rpartition(b"\n")
    if not code.startswith(b"2"):
        if b"This Edit has been deleted" in text:
            raise EditDeleted()
        sys.exit(f"{method} {url.split('?')[0]}: {code.decode()} {text.decode()}")
    return json.loads(text) if text.strip() else {}


def upload(token, bundle_path, track, status):
    edit = call("POST", f"{API}/edits", b"{}", token=token)["id"]
    with open(bundle_path, "rb") as f:
        bundle = call("POST", f"{UPLOAD}/edits/{edit}/bundles?uploadType=media", f.read(),
                      "application/octet-stream", token)
    version = bundle["versionCode"]
    release = {"releases": [{"versionCodes": [str(version)], "status": status}]}
    call("PUT", f"{API}/edits/{edit}/tracks/{track}", json.dumps(release).encode(), token=token)
    call("POST", f"{API}/edits/{edit}:commit", token=token)
    print(f"Uploaded version {version} to {track} as {status}")


def main():
    account_path, bundle_path, track, status = sys.argv[1:5]
    with open(account_path) as f:
        token = access_token(json.load(f))
    for attempt in range(5):
        try:
            return upload(token, bundle_path, track, status)
        except EditDeleted:
            print("Another upload took the edit, starting over")
            time.sleep(30 * (attempt + 1))
    sys.exit("Gave up after five deleted edits")


if __name__ == "__main__":
    main()
