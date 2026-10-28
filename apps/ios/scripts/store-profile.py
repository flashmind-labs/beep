#!/usr/bin/env python3
"""Fetch an App Store profile for a bundle ID, creating it when missing.

Usage: store-profile.py BUNDLE_ID REFERENCE_PROFILE OUTPUT [CAPABILITY...]

The profile is signed by the same distribution certificate as
REFERENCE_PROFILE. Each CAPABILITY (an App Store Connect capability type such
as PUSH_NOTIFICATIONS) is turned on for the bundle ID first; Apple then marks
older profiles invalid, so a new one is made. It reads the App Store Connect key
from IOS_API_KEY_PATH, APPLE_API_KEY_ID and APPLE_API_ISSUER_ID.
"""

import base64
import json
import os
import plistlib
import subprocess
import sys
import time
import urllib.error
import urllib.request

API = "https://api.appstoreconnect.apple.com/v1"


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def der_to_raw(der):
    """openssl prints an ES256 signature as DER; a JWT wants r || s."""
    assert der[0] == 0x30
    i = 3 if der[1] & 0x80 else 2
    parts = []
    for _ in range(2):
        assert der[i] == 0x02
        size = der[i + 1]
        parts.append(der[i + 2:i + 2 + size].lstrip(b"\0").rjust(32, b"\0"))
        i += 2 + size
    return b"".join(parts)


def token():
    header = {"alg": "ES256", "kid": os.environ["APPLE_API_KEY_ID"], "typ": "JWT"}
    now = int(time.time())
    claims = {"iss": os.environ["APPLE_API_ISSUER_ID"], "iat": now, "exp": now + 900,
              "aud": "appstoreconnect-v1"}
    signing_input = f"{b64url(json.dumps(header).encode())}.{b64url(json.dumps(claims).encode())}"
    der = subprocess.run(["openssl", "dgst", "-sha256", "-sign", os.environ["IOS_API_KEY_PATH"]],
                         input=signing_input.encode(), capture_output=True, check=True).stdout
    return f"{signing_input}.{b64url(der_to_raw(der))}"


def call(method, path, body=None):
    request = urllib.request.Request(
        path if path.startswith("http") else API + path, method=method,
        data=json.dumps(body).encode() if body else None,
        headers={"Authorization": f"Bearer {token()}", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(request) as response:
            text = response.read()
    except urllib.error.HTTPError as error:
        sys.exit(f"{method} {path}: {error.code} {error.read().decode()}")
    return json.loads(text) if text else None


def reference_certificates(path):
    decoded = subprocess.run(["security", "cms", "-D", "-i", path],
                             capture_output=True, check=True).stdout
    return set(plistlib.loads(decoded)["DeveloperCertificates"])


def main():
    bundle, reference, output, *capabilities = sys.argv[1:]
    wanted = reference_certificates(reference)

    found = call("GET", f"/bundleIds?filter[identifier]={bundle}&limit=200")["data"]
    found = [b for b in found if b["attributes"]["identifier"] == bundle]
    if found:
        bundle_id = found[0]["id"]
    else:
        bundle_id = call("POST", "/bundleIds", {"data": {"type": "bundleIds", "attributes": {
            "identifier": bundle, "name": bundle.replace(".", " "), "platform": "IOS"}}})["data"]["id"]

    enabled = {c["attributes"]["capabilityType"]
               for c in call("GET", f"/bundleIds/{bundle_id}/bundleIdCapabilities")["data"]}
    for capability in capabilities:
        if capability not in enabled:
            call("POST", "/bundleIdCapabilities", {"data": {
                "type": "bundleIdCapabilities",
                "attributes": {"capabilityType": capability},
                "relationships": {"bundleId": {"data": {"type": "bundleIds", "id": bundle_id}}}}})

    certificates = call("GET", "/certificates?filter[certificateType]=DISTRIBUTION,IOS_DISTRIBUTION&limit=200")
    certificate = next((c["id"] for c in certificates["data"]
                        if base64.b64decode(c["attributes"]["certificateContent"]) in wanted), None)
    if certificate is None:
        sys.exit("The reference profile's certificate is not on this team")

    name = f"{bundle} App Store"
    profile = None
    for candidate in call("GET", f"/bundleIds/{bundle_id}/profiles?limit=200")["data"]:
        attributes = candidate["attributes"]
        if attributes["profileType"] != "IOS_APP_STORE" or attributes["name"] != name:
            continue
        signers = call("GET", f"/profiles/{candidate['id']}/certificates")["data"]
        if attributes["profileState"] == "ACTIVE" and any(s["id"] == certificate for s in signers):
            profile = candidate
        else:
            # Expired or made for an older certificate: replace it under the same name.
            call("DELETE", f"/profiles/{candidate['id']}")
    if profile is None:
        profile = call("POST", "/profiles", {"data": {
            "type": "profiles",
            "attributes": {"name": name, "profileType": "IOS_APP_STORE"},
            "relationships": {
                "bundleId": {"data": {"type": "bundleIds", "id": bundle_id}},
                "certificates": {"data": [{"type": "certificates", "id": certificate}]},
            }}})["data"]

    with open(output, "wb") as file:
        file.write(base64.b64decode(profile["attributes"]["profileContent"]))
    print(profile["attributes"]["uuid"])


if __name__ == "__main__":
    main()
