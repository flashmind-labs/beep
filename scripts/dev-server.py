#!/usr/bin/env python3
"""Start a local fixture or OpenRouter-backed beep server; generate private Android profiles."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parent.parent
os.chdir(ROOT)
os.umask(0o077)
parser = argparse.ArgumentParser()
parser.add_argument("--openrouter-key-file", type=Path, help="Read an OpenRouter key from a private file outside git")
parser.add_argument("--firecrawl-key-file", type=Path, help="Enable web search using a private Firecrawl key file")
parser.add_argument("--model", default="z-ai/glm-5.3-flash", help="OpenRouter model ID (default: z-ai/glm-5.3-flash)")
args = parser.parse_args()
openrouter_key = None
if args.openrouter_key_file:
    openrouter_key = args.openrouter_key_file.expanduser().read_text().strip()
    if not openrouter_key or any(c.isspace() for c in openrouter_key):
        raise SystemExit("Key file must contain only the OpenRouter API key")

firecrawl_key = None
if args.firecrawl_key_file:
    firecrawl_key = args.firecrawl_key_file.expanduser().read_text().strip()
    if not firecrawl_key or any(c.isspace() for c in firecrawl_key):
        raise SystemExit("Key file must contain only the Firecrawl API key")

Path(".dev").mkdir(exist_ok=True)
config = f'''secret_key = ".dev/iroh-key"
database_url = "postgres://beep:beep@127.0.0.1:55432/beep"
log_dir = ".dev/logs"
model = "openai:beep-test"
provider_url = "http://127.0.0.1:18080/"
'''
if openrouter_key:
    config = config.replace('model = "openai:beep-test"', "model = " + json.dumps("openrouter:" + args.model))
    config = config.replace('provider_url = "http://127.0.0.1:18080/"', "provider_key = " + json.dumps(openrouter_key))
if firecrawl_key:
    config = "firecrawl_key = " + json.dumps(firecrawl_key) + "\n" + config
Path(".dev/android-server.toml").write_text(config)

subprocess.run(["cargo", "build", "-q", "-p", "beep-server", "--bins"], check=True)

# Read the node ID from the iroh key (generated on first run) for the testing profile.
key_path = Path(".dev/iroh-key")
assets = Path(".dev/android-assets")
assets.mkdir(exist_ok=True)
if key_path.exists():
    from hashlib import sha512
    key_hex = key_path.read_text().strip()
    key_bytes = bytes.fromhex(key_hex)
    # ed25519: public key is the last 32 bytes of SHA-512(seed)
    h = sha512(key_bytes[:32]).digest()
    pub_bytes = bytearray(h[:32])
    # ed25519 clamping
    pub_bytes[0] &= 248
    pub_bytes[31] &= 127
    pub_bytes[31] |= 64
    # The actual public key derivation needs the real ed25519 scalar multiply,
    # so just let the server print it. We write a placeholder and update after start.
    debug_profile = {"node_id": ""}
    (assets / "testing-profile.json").write_text(json.dumps(debug_profile, indent=2))
else:
    debug_profile = {"node_id": ""}
    (assets / "testing-profile.json").write_text(json.dumps(debug_profile, indent=2))

children = []
try:
    with open(".dev/android-model.log", "a") as model_log, open(".dev/android-server.log", "a") as server_log:
        if not openrouter_key:
            children.append(subprocess.Popen(["python3", "scripts/test-model.py"], stdout=model_log, stderr=model_log))
        children.append(subprocess.Popen(["target/debug/beep-server", ".dev/android-server.toml"], stdout=server_log, stderr=server_log))
        print(("OpenRouter" if openrouter_key else "TEST provider") + " and server started. Profiles are private files in .dev/.", flush=True)
        while all(child.poll() is None for child in children):
            time.sleep(1)
        raise SystemExit("A development process exited; inspect .dev/android-{model,server}.log")
except KeyboardInterrupt:
    pass
finally:
    for child in children:
        if child.poll() is None:
            child.send_signal(signal.SIGINT)
    for child in children:
        try:
            child.wait(timeout=12)
        except subprocess.TimeoutExpired:
            child.kill()
