#!/usr/bin/env python3
"""Read release notes, check pending entries, or freeze them into a version.

Pending entries are files in changelogs/unreleased/<app>/, one per change, so
branches never edit the same lines. Freezing a version collects them into the
app's changelog and deletes them.
"""

import argparse
from datetime import datetime, timezone
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parent.parent
VERSION = re.compile(r"0\.[0-9]+\.[0-9]+")
SECTION = re.compile(r"^## ([^\n]+)\n(.*?)(?=^## |\Z)", re.MULTILINE | re.DOTALL)
SUBSECTION = re.compile(r"^### ([^\n]+)\n", re.MULTILINE)
SECTIONS = ("Added", "Changed", "Fixed", "Security", "Performance")
ENTRY = re.compile(r"(?m)^- \S")


def notes(text: str, label: str) -> str:
    matches = [body.strip() for heading, body in SECTION.findall(text)
               if label in re.findall(r"\[([^]]+)\]", heading)]

    if len(matches) != 1 or not ENTRY.search(matches[0]):
        raise ValueError(f"Missing or duplicate release notes for {label}")

    return matches[0]


def pending_dir(changelog: Path) -> Path:
    """Where changelogs/<app>.md keeps its pending entries."""
    return changelog.parent / "unreleased" / changelog.stem


def entries(changelog: Path) -> list[Path]:
    folder = pending_dir(changelog)
    return sorted(folder.glob("*.md")) if folder.is_dir() else []


def read_entry(path: Path) -> dict[str, str]:
    """An entry's sections by name, each a list of `- ` items."""
    parts = SUBSECTION.split(path.read_text())

    if parts[0].strip():
        raise ValueError(f"{path}: text before the first ### section")

    sections = {}
    for name, body in zip(parts[1::2], parts[2::2]):
        name = name.strip()

        if name not in SECTIONS:
            raise ValueError(f"{path}: unknown section {name!r}, expected one of {', '.join(SECTIONS)}")

        if name in sections:
            raise ValueError(f"{path}: section {name} appears twice")

        if not ENTRY.match(body.strip()):
            raise ValueError(f"{path}: section {name} has no `- ` items")

        sections[name] = body.strip()

    if not sections:
        raise ValueError(f"{path}: no ### sections")

    return sections


def unreleased(changelog: Path) -> str:
    """The pending entries as one version's notes, sections in their order."""
    collected = {name: [] for name in SECTIONS}
    for path in entries(changelog):
        for name, body in read_entry(path).items():
            collected[name].append(body)

    body = "\n\n".join(f"### {name}\n\n" + "\n".join(items) for name, items in collected.items() if items)

    if not body:
        raise ValueError(f"No pending release notes in {pending_dir(changelog)}")

    return body


def freeze(path: Path, version: str) -> list[Path]:
    """Writes the pending entries as `version` and deletes them; gives the files deleted."""
    if not VERSION.fullmatch(version):
        raise ValueError("Expected a 0.x release version")

    text = path.read_text()
    headings = [label for heading, _ in SECTION.findall(text)
                for label in re.findall(r"\[([^]]+)\]", heading)]

    if version in headings:
        raise ValueError("Release notes already exist for this version")

    if "Unreleased" in headings:
        raise ValueError(f"{path} has an [Unreleased] section: move it into {pending_dir(path)}")

    body = unreleased(path)
    date = datetime.now(timezone.utc).date().isoformat()
    entry = f"## [{version}] - {date}\n\n{body}\n\n"
    first = SECTION.search(text)
    text = text[:first.start()] + entry + text[first.start():] if first else text.rstrip("\n") + "\n\n" + entry

    path.write_text(text)
    deleted = entries(path)
    for entry_path in deleted:
        entry_path.unlink()

    return deleted


def check(root: Path) -> None:
    """Every pending entry reads, and belongs to an app with a changelog."""
    folder = root / "changelogs/unreleased"

    for path in sorted(folder.glob("*/*.md")) if folder.is_dir() else []:
        if not (root / "changelogs" / f"{path.parent.name}.md").is_file():
            raise ValueError(f"{path}: no changelogs/{path.parent.name}.md")

        read_entry(path)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["notes", "freeze", "check"])
    parser.add_argument("version", nargs="?")
    parser.add_argument("--path", type=Path, default=ROOT / "changelogs/desktop.md")
    args = parser.parse_args()

    if args.command == "check":
        check(ROOT)
    elif args.version is None:
        parser.error(f"{args.command} needs a version")
    elif args.command == "freeze":
        freeze(args.path, args.version)
    else:
        print(notes(args.path.read_text(), args.version))


if __name__ == "__main__":
    try:
        main()
    except ValueError as error:
        raise SystemExit(f"changelog: {error}")
