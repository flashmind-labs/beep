#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# ///
"""Bump selected apps together and publish their release tags."""

import argparse
import json
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True, order=True)
class Version:
    major: int
    minor: int
    patch: int

    @classmethod
    def parse(cls, value: str):
        if not re.fullmatch(r"0\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", value):
            raise ValueError(f"Unsupported version {value!r}: releases must stay on 0.x")

        return cls(*map(int, value.split(".")))

    def bump(self, level: str):
        return Version(0, self.minor + 1, 0) if level == "minor" else Version(0, self.minor, self.patch + 1)

    def __str__(self):
        return f"{self.major}.{self.minor}.{self.patch}"


@dataclass(frozen=True)
class App:
    name: str
    prefix: str
    manifest: str
    package: str | None = None

    def version(self, root: Path) -> Version:
        path = root / self.manifest
        data = tomllib.loads(path.read_text()) if self.package else json.loads(path.read_text())
        return Version.parse(data["package"]["version"] if self.package else data["version"])

    def write_version(self, root: Path, version: Version):
        path = root / self.manifest
        text = path.read_text()

        if self.package:
            # Parse with tomllib, then preserve the manifest's formatting and comments.
            self.version(root)
            text, count = re.subn(
                r'(?ms)(^\[package\]\s*\n(?:(?!^\[).)*?^version\s*=\s*)"[^"]+"',
                lambda match: f'{match[1]}"{version}"', text, count=1,
            )
            if count != 1:
                raise ValueError(f"Cannot update {self.manifest}")
        else:
            data = json.loads(text)
            data["version"] = str(version)
            text = json.dumps(data, indent=2) + "\n"

        path.write_text(text)


APPS = (
    App("desktop", "desktop", "apps/desktop/src-tauri/Cargo.toml", "beep-desktop"),
    App("beep-node", "node", "crates/node/Cargo.toml", "beep-node"),
    App("android", "android", "apps/desktop/src-tauri/tauri.android.conf.json"),
    App("ios", "ios", "apps/desktop/src-tauri/tauri.ios.conf.json"),
)


def run(root: Path, *args: str) -> str:
    result = subprocess.run(args, cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    if result.returncode:
        # Git errors may contain authenticated remote URLs; keep diagnostics local to Git.
        if args[0] == "git" and args[1] in {"fetch", "push", "remote"}:
            raise ValueError(f"git {args[1]} failed; check the remote and your access")
        raise ValueError(result.stderr.strip() or f"{args[0]} failed")

    return result.stdout.strip()


def git(root: Path, *args: str) -> str:
    return run(root, "git", *args)


def ancestor(root: Path, before: str, after: str) -> bool:
    result = subprocess.run(["git", "merge-base", "--is-ancestor", before, after], cwd=root)

    if result.returncode not in (0, 1):
        raise ValueError("Cannot compare release history")

    return result.returncode == 0


def select(names: list[str]) -> list[App]:
    names = ["beep-node" if name == "node" else name for name in names]

    if names == ["all"]:
        return list(APPS)
    if not names or len(set(names)) != len(names):
        raise ValueError("Select at least one app, with no duplicates")
    if set(names) - {app.name for app in APPS}:
        raise ValueError("Apps: desktop, beep-node (or node), android, ios; use all for every app")

    return [app for app in APPS if app.name in names]


def plan(root: Path, apps: list[App], level: str) -> list[tuple[App, Version]]:
    releases = []

    for app in apps:
        tags = []

        for tag in git(root, "tag", "--list", f"{app.prefix}-v*").splitlines():
            value = tag.removeprefix(f"{app.prefix}-v")
            if re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", value):
                tags.append((Version.parse(value), tag))

        current = app.version(root)

        if tags:
            latest, tag = max(tags)
            if not ancestor(root, tag, "HEAD"):
                raise ValueError(f"{tag} is missing from this checkout; merge its release history into main first")
            current = max(current, latest)

        releases.append((app, current.bump(level)))

    return releases


def clean_main(root: Path):
    if git(root, "branch", "--show-current") != "main":
        raise ValueError("Run releases from the main checkout on branch main")
    if git(root, "status", "--porcelain"):
        raise ValueError("Commit or stash your changes before releasing")


def refresh(root: Path, remote: str) -> list[str]:
    # origin may have separate push URLs for the source mirror and the build host.
    urls = list(dict.fromkeys(git(root, "remote", "get-url", "--push", "--all", remote).splitlines()))

    for url in urls:
        git(root, "fetch", "--no-recurse-submodules", "--tags", url, "refs/heads/main")
        if not ancestor(root, "FETCH_HEAD", "HEAD"):
            raise ValueError("Published main is missing from this checkout; merge it into main before releasing")

    return urls


def update_locks(tree: Path, apps: list[App]) -> list[str]:
    files = []

    if any(app.name == "beep-node" for app in apps):
        run(tree, "cargo", "metadata", "--offline", "--format-version", "1")
        files.append("Cargo.lock")

    if any(app.package for app in apps):
        run(tree / "apps/desktop/src-tauri", "cargo", "metadata", "--offline", "--format-version", "1")
        files.append("apps/desktop/src-tauri/Cargo.lock")

    return files


def release(root: Path, releases: list[tuple[App, Version]], urls: list[str]):
    head = git(root, "rev-parse", "HEAD")
    labels = [f"{app.name} {version}" for app, version in releases]
    tags = [f"{app.prefix}-v{version}" for app, version in releases]
    branch = "release-" + "-".join(tags)
    tree = root / ".worktrees" / branch
    git(root, "worktree", "add", "-b", branch, str(tree), head)
    committed = False

    try:
        apps = [app for app, _ in releases]

        for app, version in releases:
            app.write_version(tree, version)

        files = [app.manifest for app in apps] + update_locks(tree, apps)
        changed = set(git(tree, "diff", "--name-only").splitlines())

        if changed - set(files):
            raise ValueError("Unexpected files changed while preparing the release")

        git(tree, "add", "--", *files)
        git(tree, "commit", "-m", "Release " + ", ".join(labels))
        committed = True
        release_commit = git(tree, "rev-parse", "HEAD")
        clean_main(root)

        if git(root, "rev-parse", "HEAD") != head:
            raise ValueError("Main changed during release preparation; merge the release branch manually")

        git(root, "merge", "--ff-only", branch)

        for tag, label in zip(tags, labels):
            git(root, "tag", "-a", tag, release_commit, "-m", label)

        git(root, "worktree", "remove", str(tree))
        git(root, "branch", "-d", branch)
    except Exception:
        if committed:
            print(f"Release commit preserved on {branch}; inspect it before retrying", file=sys.stderr)
        else:
            # This worktree contains only generated version changes, never user edits.
            git(root, "worktree", "remove", "--force", str(tree))
            git(root, "branch", "-d", branch)
        raise

    for index, url in enumerate(urls, 1):
        print(f"Publishing main and {len(tags)} tags to destination {index}/{len(urls)}", flush=True)
        try:
            git(root, "push", "--atomic", url, f"{release_commit}:refs/heads/main", *[f"refs/tags/{tag}" for tag in tags])
        except ValueError:
            print("The release commit and tags are preserved. Retry publishing the same tags; do not bump again.", file=sys.stderr)
            raise

    print("Released " + ", ".join(labels))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("level", choices=["patch", "minor"])
    parser.add_argument("apps", nargs="+")
    parser.add_argument("--remote", default="origin", help="remote whose push URLs publish the release (default: origin)")
    parser.add_argument("--dry-run", action="store_true", help="fetch and validate, then print versions without committing or publishing")
    args = parser.parse_args()
    apps = select(args.apps)
    root = Path(git(Path.cwd(), "rev-parse", "--show-toplevel"))
    clean_main(root)
    urls = refresh(root, args.remote)
    releases = plan(root, apps, args.level)

    for app, version in releases:
        print(f"{app.name}: {app.version(root)} -> {version} ({app.prefix}-v{version})", flush=True)

    if not args.dry_run:
        release(root, releases, urls)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError) as error:
        print(f"Release failed: {error}", file=sys.stderr)
        sys.exit(1)
