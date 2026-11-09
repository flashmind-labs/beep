#!/usr/bin/env python3
"""Stage Windows or Linux desktop installers and their updater manifest."""

import hashlib
import json
import re
import shutil
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path


def stage(platform: str, source: Path, output: Path) -> None:
    if platform not in {"linux", "windows"}:
        raise ValueError(f"Unsupported desktop platform: {platform}")

    cargo = Path(__file__).resolve().parent.parent / "apps/desktop/src-tauri/Cargo.toml"
    version = tomllib.loads(cargo.read_text())["package"]["version"]

    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("Invalid desktop version")

    suffixes = [".AppImage", ".deb"] if platform == "linux" else [".exe"]
    files = output / "files"
    manifest = output / "manifest"
    files.mkdir(parents=True, exist_ok=True)
    manifest.mkdir(parents=True, exist_ok=True)
    updater = None
    checksums = []

    for suffix in suffixes:
        matches = list(source.glob(f"*{suffix}"))

        if len(matches) != 1:
            raise ValueError(f"Expected one {suffix} installer, found {len(matches)}")

        installer = matches[0]
        name = f"Beep-{version}-x86_64{suffix}"
        shutil.copyfile(installer, files / name)
        with installer.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        checksums.append(f"{digest}  {name}\n")

        if suffix != ".deb":
            signature_path = installer.with_name(installer.name + ".sig")
            signature = signature_path.read_text().strip()

            if not signature:
                raise ValueError("The updater signature is empty")

            shutil.copyfile(signature_path, files / (name + ".sig"))
            updater = {
                "signature": signature,
                "url": f"https://releases.flashmind.com/beep/{platform}/{name}",
            }

    (files / f"SHA256SUMS-{platform}").write_text("".join(checksums))
    key = "linux-x86_64" if platform == "linux" else "windows-x86_64"
    (manifest / "latest.json").write_text(json.dumps({
        "version": version,
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {key: updater},
    }, indent=2) + "\n")


if __name__ == "__main__":
    stage(sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]))
