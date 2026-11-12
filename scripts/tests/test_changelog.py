import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from xml.etree import ElementTree as ET


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from changelog import check, freeze, notes, unreleased

spec = importlib.util.spec_from_file_location("appcast", ROOT / "scripts/write-macos-appcast.py")
appcast = importlib.util.module_from_spec(spec)
spec.loader.exec_module(appcast)


def app_tree(root: Path, changelog: str, entries: dict[str, str]) -> Path:
    """A changelog at changelogs/app.md with pending entry files by name."""
    path = root / "changelogs/app.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(changelog)
    folder = root / "changelogs/unreleased/app"
    folder.mkdir(parents=True, exist_ok=True)

    for name, text in entries.items():
        (folder / name).write_text(text)

    return path


class ReleaseNotes(unittest.TestCase):
    def test_freeze_collects_entries_in_section_order_and_deletes_them(self):
        published = "# Changelog\n\nIntro.\n\n## [0.1.0] - 2026-01-01\n\n- First release.\n"
        entries = {
            "b-sync.md": "### Fixed\n\n- App: reconnect after sleep,\n  even on battery.\n",
            "a-search.md": "### Fixed\n\n- App: search finds folders.\n\n### Added\n\n- App: search.\n",
        }

        with tempfile.TemporaryDirectory() as directory:
            path = app_tree(Path(directory), published, entries)
            deleted = freeze(path, "0.2.0")
            released = path.read_text()

            self.assertEqual(notes(released, "0.2.0"),
                             "### Added\n\n- App: search.\n\n### Fixed\n\n- App: search finds folders.\n"
                             "- App: reconnect after sleep,\n  even on battery.")
            self.assertEqual(notes(released, "0.1.0"), "- First release.")
            self.assertTrue(released.startswith("# Changelog\n\nIntro.\n\n## [0.2.0] - "))
            self.assertEqual(sorted(p.name for p in deleted), sorted(entries))
            self.assertFalse(any(p.exists() for p in deleted))

            with self.assertRaises(ValueError):
                freeze(path, "0.2.1")
            self.assertEqual(path.read_text(), released)

    def test_first_release_goes_after_the_intro(self):
        with tempfile.TemporaryDirectory() as directory:
            path = app_tree(Path(directory), "# Changelog\n\nIntro.\n", {"x.md": "### Added\n\n- New.\n"})
            freeze(path, "0.1.0")
            self.assertEqual(notes(path.read_text(), "0.1.0"), "### Added\n\n- New.")

    def test_releasing_one_app_preserves_other_apps_pending_entries(self):
        from release import APPS
        self.assertEqual(len({app.changelog for app in APPS}), len(APPS))

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            for app in APPS:
                path = root / app.changelog
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("# Changelog\n")
                folder = root / "changelogs/unreleased" / path.stem
                folder.mkdir(parents=True)
                (folder / "x.md").write_text("### Added\n\n- New behavior.\n")

            freeze(root / APPS[0].changelog, "0.2.0")

            for app in APPS[1:]:
                self.assertEqual(unreleased(root / app.changelog), "### Added\n\n- New behavior.")

    def test_rejects_bad_or_missing_entries_without_writing(self):
        for entry in ["- No section.\n", "### Removed\n\n- Gone.\n", "### Added\n\nNo items.\n",
                      "### Added\n\n- One.\n\n### Added\n\n- Two.\n", "Intro.\n\n### Added\n\n- One.\n"]:
            with self.subTest(entry=entry), tempfile.TemporaryDirectory() as directory:
                path = app_tree(Path(directory), "# Changelog\n", {"x.md": entry})

                with self.assertRaises(ValueError):
                    freeze(path, "0.2.0")
                self.assertEqual(path.read_text(), "# Changelog\n")

        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                freeze(app_tree(Path(directory), "# Changelog\n", {}), "0.2.0")

    def test_rejects_a_left_over_unreleased_section(self):
        text = "# Changelog\n\n## [Unreleased]\n\n- Edited in place.\n"

        with tempfile.TemporaryDirectory() as directory:
            path = app_tree(Path(directory), text, {"x.md": "### Added\n\n- New.\n"})

            with self.assertRaises(ValueError):
                freeze(path, "0.2.0")
            self.assertEqual(path.read_text(), text)

    def test_rejects_missing_empty_and_duplicate_notes(self):
        for text in ["# Changelog", "## [0.1.0]\n\n### Added\n",
                     "## [0.1.0]\n- One.\n## [0.1.0]\n- Two.\n"]:
            with self.subTest(text=text), self.assertRaises(ValueError):
                notes(text, "0.1.0")

    def test_pending_entries_in_the_repository_read(self):
        check(ROOT)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app_tree(root, "# Changelog\n", {})
            stray = root / "changelogs/unreleased/nothing/x.md"
            stray.parent.mkdir(parents=True)
            stray.write_text("### Added\n\n- New.\n")

            with self.assertRaises(ValueError):
                check(root)

    def test_feed_preserves_markdown_and_escapes_xml(self):
        info = {"CFBundleVersion": "0.2.0", "CFBundleShortVersionString": "0.2.0",
                "SUFeedURL": f"{appcast.BASE_URL}/appcast.xml"}
        body = "### Fixed\n\n- Keep <notes> & release details."

        with tempfile.TemporaryDirectory() as directory:
            dmg = Path(directory) / "Beep-0.2.0-arm64.dmg"
            dmg.write_bytes(b"signed disk image")
            feed = ET.fromstring(appcast.appcast(info, "0.2.0", dmg, "signature", body))
            item = feed.find("channel/item")
            self.assertEqual(item.findtext("description"), body)
            self.assertEqual(item.find("description").get(f"{{{appcast.SPARKLE}}}format"), "markdown")
            self.assertEqual(item.find("enclosure").get("length"), str(dmg.stat().st_size))
            self.assertEqual(item.findtext(f"{{{appcast.SPARKLE}}}version"), "0.2.0")

            with self.assertRaises(ValueError):
                appcast.appcast(info, "0.2.1", dmg, "signature", body)

    def test_derives_sparkle_public_key_from_known_ed25519_seed(self):
        # RFC 8032 test vector 1, a public test key.
        import base64
        seed = base64.b64encode(bytes.fromhex(
            "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")).decode()
        expected = base64.b64encode(bytes.fromhex(
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")).decode()
        self.assertEqual(appcast.public_key(seed), expected)

        with self.assertRaises(ValueError):
            appcast.public_key("c2hvcnQ=")


if __name__ == "__main__":
    unittest.main()
