#!/usr/bin/env python3
"""Read release notes or freeze Unreleased for an app version."""

import argparse
from datetime import datetime, timezone
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parent.parent
VERSION = re.compile(r"0\.[0-9]+\.[0-9]+")
SECTION = re.compile(r"^## ([^\n]+)\n(.*?)(?=^## |\Z)", re.MULTILINE | re.DOTALL)


def notes(text: str, label: str) -> str:
    matches = [body.strip() for heading, body in SECTION.findall(text)
               if label in re.findall(r"\[([^]]+)\]", heading)]

    if len(matches) != 1 or not re.search(r"(?m)^- \S", matches[0]):
        raise ValueError(f"Missing or duplicate release notes for {label}")

    return matches[0]


def freeze(path: Path, version: str) -> None:
    if not VERSION.fullmatch(version):
        raise ValueError("Expected a 0.x release version")

    text = path.read_text()
    body = notes(text, "Unreleased")
    headings = [label for heading, _ in SECTION.findall(text)
                for label in re.findall(r"\[([^]]+)\]", heading)]

    if version in headings:
        raise ValueError("Release notes already exist for this version")

    date = datetime.now(timezone.utc).date().isoformat()
    replacement = f"## [Unreleased]\n\n## [{version}] - {date}\n\n{body}\n\n"
    path.write_text(SECTION.sub(lambda match: replacement if "[Unreleased]" == match[1] else match[0], text))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["notes", "freeze"])
    parser.add_argument("version")
    parser.add_argument("--path", type=Path, default=ROOT / "changelogs/desktop.md")
    args = parser.parse_args()

    if args.command == "freeze":
        freeze(args.path, args.version)
    else:
        print(notes(args.path.read_text(), args.version))


if __name__ == "__main__":
    main()
