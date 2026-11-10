import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from xml.etree import ElementTree as ET


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from changelog import freeze, notes

spec = importlib.util.spec_from_file_location("appcast", ROOT / "scripts/write-macos-appcast.py")
appcast = importlib.util.module_from_spec(spec)
spec.loader.exec_module(appcast)


class ReleaseNotes(unittest.TestCase):
    def test_freezes_snapshot_and_preserves_published_notes(self):
        original = "# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- Desktop: reconnect after sleep.\n\n## [0.1.0] - 2026-01-01\n\n- First release.\n"

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "CHANGELOG.md"
            path.write_text(original)
            freeze(path, "0.2.0")
            released = path.read_text()
            self.assertEqual(notes(released, "0.2.0"), notes(original, "Unreleased"))
            self.assertEqual(notes(released, "0.1.0"), "- First release.")
            self.assertIn("## [Unreleased]\n\n", released)

            with self.assertRaises(ValueError):
                freeze(path, "0.2.1")
            self.assertEqual(path.read_text(), released)

    def test_releasing_one_app_preserves_other_apps_pending_notes(self):
        from release import APPS
        self.assertEqual(len({app.changelog for app in APPS}), len(APPS))

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original = "## [Unreleased]\n\n### Added\n\n- New behavior.\n"

            for app in APPS:
                path = root / app.changelog
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(original)

            freeze(root / APPS[0].changelog, "0.2.0")

            for app in APPS[1:]:
                self.assertEqual((root / app.changelog).read_text(), original)

    def test_rejects_missing_empty_and_duplicate_notes(self):
        for text in ["# Changelog", "## [Unreleased]\n\n### Added\n",
                     "## [Unreleased]\n- One.\n## [Unreleased]\n- Two.\n"]:
            with self.subTest(text=text), self.assertRaises(ValueError):
                notes(text, "Unreleased")

    def test_rejects_reusing_a_published_tag_without_writing(self):
        text = "## [Unreleased]\n- New.\n## [0.1.0]\n- Old.\n"

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "CHANGELOG.md"
            path.write_text(text)

            with self.assertRaises(ValueError):
                freeze(path, "0.1.0")
            self.assertEqual(path.read_text(), text)

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
