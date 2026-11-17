import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/cargo-plain.sh"


class PlainCargo(unittest.TestCase):
    def test_forced_colors_are_removed_without_changing_app_arguments(self):
        target = ROOT / "target"
        target.mkdir(exist_ok=True)

        with tempfile.TemporaryDirectory(dir=target) as directory:
            root = Path(directory)
            cargo = root / "cargo"
            cargo.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\"\nexit 42\n")
            cargo.chmod(0o755)
            env = dict(os.environ, PATH=f"{root}{os.pathsep}{os.environ['PATH']}")
            result = subprocess.run(
                [str(SCRIPT), "run", "--color=auto", "--features", "some feature",
                 "--color", "always", "--", "--color", "always"],
                env=env, text=True, capture_output=True,
            )

        self.assertEqual(result.returncode, 42, result.stderr)
        self.assertEqual(result.stdout.splitlines(), [
            "--color", "never", "run", "--features", "some feature",
            "--", "--color", "always",
        ])

    def test_real_cargo_diagnostics_have_no_ansi_even_when_color_is_forced(self):
        result = subprocess.run(
            [str(SCRIPT), "check", "--color", "always", "-p", "nonexistent-color-probe"],
            cwd=ROOT, env=dict(os.environ, CARGO_TERM_COLOR="always"),
            text=True, capture_output=True,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("nonexistent-color-probe", result.stderr)
        self.assertNotIn("\x1b", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
