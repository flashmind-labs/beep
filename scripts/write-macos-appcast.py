#!/usr/bin/env python3
"""Sign a notarized macOS disk image and write its Sparkle feed."""

import argparse
import base64
from datetime import datetime, timezone
import os
from pathlib import Path
import plistlib
import re
import subprocess
import sys
from xml.etree import ElementTree as ET

from changelog import notes


SPARKLE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
ET.register_namespace("sparkle", SPARKLE)
BASE_URL = "https://releases.flashmind.com/beep/macos"


def public_key(seed: str) -> str:
    raw = base64.b64decode(seed, validate=True)

    if len(raw) != 32:
        raise ValueError("Sparkle signing key must be a base64 Ed25519 seed")

    # Apple's LibreSSL cannot derive Ed25519 keys; release hosts use OpenSSL 3.
    openssl = "openssl"

    if sys.platform == "darwin":
        for prefix in (Path("/opt/homebrew"), Path("/usr/local")):
            candidate = prefix / "opt/openssl@3/bin/openssl"

            if candidate.is_file():
                openssl = str(candidate)
                break

    result = subprocess.run(
        [openssl, "pkey", "-inform", "DER", "-pubout", "-outform", "DER"],
        input=bytes.fromhex("302e020100300506032b657004220420") + raw,
        capture_output=True, check=True,
    ).stdout
    prefix = bytes.fromhex("302a300506032b6570032100")

    if not result.startswith(prefix) or len(result) != len(prefix) + 32:
        raise ValueError("Cannot derive the Sparkle public key")

    return base64.b64encode(result[len(prefix):]).decode()


def appcast(info: dict, version: str, dmg: Path, signature: str, body: str) -> bytes:
    arch = dmg.name.removeprefix(f"Beep-{version}-").removesuffix(".dmg")

    if arch not in {"arm64", "x86_64"} or info.get("CFBundleShortVersionString") != version:
        raise ValueError("Disk image or app version does not match the release")
    if info.get("SUFeedURL") != f"{BASE_URL}/appcast.xml":
        raise ValueError("App update feed does not match the release")

    root = ET.Element("rss", {"version": "2.0"})
    channel = ET.SubElement(root, "channel")
    ET.SubElement(channel, "title").text = "Beep updates"
    item = ET.SubElement(channel, "item")
    ET.SubElement(item, "title").text = f"Beep {version}"
    ET.SubElement(item, "pubDate").text = datetime.now(timezone.utc).strftime("%a, %d %b %Y %H:%M:%S %z")
    ET.SubElement(item, "description", {f"{{{SPARKLE}}}format": "markdown"}).text = body
    ET.SubElement(item, f"{{{SPARKLE}}}version").text = info["CFBundleVersion"]
    ET.SubElement(item, f"{{{SPARKLE}}}shortVersionString").text = version
    ET.SubElement(item, f"{{{SPARKLE}}}minimumSystemVersion").text = "13.0"
    ET.SubElement(item, "enclosure", {
        "url": f"{BASE_URL}/{dmg.name}",
        f"{{{SPARKLE}}}edSignature": signature,
        "length": str(dmg.stat().st_size),
        "type": "application/octet-stream",
    })
    return ET.tostring(root, encoding="utf-8", xml_declaration=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--dmg", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    body = notes((root / "changelogs/desktop.md").read_text(), args.version)
    info = plistlib.loads((args.app / "Contents/Info.plist").read_bytes())
    seed = os.environ["SPARKLE_PRIVATE_KEY"].strip()

    if public_key(seed) != info.get("SUPublicEDKey"):
        raise ValueError("Sparkle signing key does not match the app")

    key_path = Path(os.environ["RUNNER_TEMP"]) / "beep-sparkle.key"
    descriptor = os.open(key_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)

    try:
        with os.fdopen(descriptor, "w") as key:
            key.write(seed)
        result = subprocess.run(
            [str(root / "apps/desktop/src-tauri/sparkle/sparkle-bin/sign_update"),
             "-f", str(key_path), str(args.dmg)],
            capture_output=True, text=True, check=True,
        ).stdout
    finally:
        key_path.unlink()

    match = re.search(r'sparkle:edSignature="([A-Za-z0-9+/=]+)" length="([0-9]+)"', result)

    if match is None or int(match[2]) != args.dmg.stat().st_size:
        raise ValueError("Sparkle signature or archive length is invalid")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(appcast(info, args.version, args.dmg, match[1], body))


if __name__ == "__main__":
    main()
