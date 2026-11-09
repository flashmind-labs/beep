import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/build-desktop-linux.sh"


class LinuxBuildRetry(unittest.TestCase):
    def run_build(self, responses):
        target = ROOT / "target"
        target.mkdir(exist_ok=True)

        with tempfile.TemporaryDirectory(dir=target) as directory:
            root = Path(directory)
            count = root / "count"
            count.write_text("0")
            (root / "tauri").write_text("""#!/bin/bash
set -eu
count=$(cat "$ATTEMPT_FILE")
count=$((count + 1))
echo "$count" > "$ATTEMPT_FILE"
printf '%s\\n' "$@" > "$ARG_FILE"
case "$RESPONSES:$count" in
  retry:1|always:*) echo 'Error failed to bundle project: `http status: 504`' >&2; exit 1 ;;
  compile:*) echo 'error: could not compile package' >&2; exit 1 ;;
esac
""")
            (root / "sleep").write_text("#!/bin/sh\nexit 0\n")
            (root / "tauri").chmod(0o755)
            (root / "sleep").chmod(0o755)
            env = dict(os.environ, PATH=f"{root}{os.pathsep}{os.environ['PATH']}",
                       ATTEMPT_FILE=str(count), ARG_FILE=str(root / "args"), RESPONSES=responses)
            result = subprocess.run(["bash", str(SCRIPT)], env=env, text=True, capture_output=True)
            return result, int(count.read_text()), (root / "args").read_text().splitlines()

    def test_retries_server_error_and_keeps_build_arguments(self):
        result, count, args = self.run_build("retry")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(count, 2)
        self.assertEqual(args, ["build", "--ci", "--bundles", "deb,appimage", "--config",
                                '{"bundle":{"createUpdaterArtifacts":true}}', "--", "--locked"])

    def test_does_not_retry_compilation_errors(self):
        result, count, _ = self.run_build("compile")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(count, 1)

    def test_stops_after_three_server_errors(self):
        result, count, _ = self.run_build("always")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(count, 3)


if __name__ == "__main__":
    unittest.main()
