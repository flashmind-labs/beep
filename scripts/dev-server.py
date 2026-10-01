#!/usr/bin/env python3
"""Start a local fixture or OpenRouter-backed server; generate private Android profiles."""
import argparse
import json
import os
from pathlib import Path
import secrets
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parent.parent
os.chdir(ROOT)
os.umask(0o077)
parser = argparse.ArgumentParser()
parser.add_argument("--rayfish", action="store_true", help="Also bind to this Mac's first active Rayfish network")
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

subprocess.run(["sh", "scripts/dev-cert.sh"], check=True)
ray_address = None
if args.rayfish:
    ray = "/Applications/Rayfish.app/Contents/MacOS/ray"
    result = json.loads(subprocess.check_output([ray, "status", "--json"]))
    networks = result.get("networks", [])
    if not result.get("active") or not networks:
        raise SystemExit("Rayfish must be connected first")
    ray_address = networks[0]["my_ipv6"]

token_path = Path(".dev/android-app-token")
if not token_path.exists():
    token_path.write_text(secrets.token_hex(32))
token = token_path.read_text().strip()
config = 'listen = "127.0.0.1:4433"\n'
if ray_address:
    config += f'additional_listen = ["[{ray_address}]:4433"]\n'
config += '''web_enabled = true
testing_user = "development"
certificate = ".dev/localhost.pem"
private_key = ".dev/localhost-key.pem"
database = ".dev/android-demo.db"
model = "openai:neobeep-test"
provider_url = "http://127.0.0.1:18080/"
[[users]]
id = "development"
token_env = "NEOBEEP_TOKEN"
'''
if openrouter_key:
    config = config.replace('model = "openai:neobeep-test"', "model = " + json.dumps("openrouter:" + args.model))
    config = config.replace('provider_url = "http://127.0.0.1:18080/"', 'provider_url = "https://openrouter.ai/api/v1/"\nprovider_key_env = "OPENROUTER_API_KEY"')
if firecrawl_key:
    config = 'firecrawl_key_env = "FIRECRAWL_API_KEY"\n' + config
Path(".dev/android-server.toml").write_text(config)
cert = Path(".dev/localhost.pem").read_text()
profile = {"address": "10.0.2.2:4433", "hostname": "localhost", "token": token, "ca": cert}
Path(".dev/android-emulator.json").write_text(json.dumps(profile, indent=2))
if ray_address:
    profile["address"] = f"[{ray_address}]:4433"
    Path(".dev/android-rayfish.json").write_text(json.dumps(profile, indent=2))
    # Debug APK bootstrap includes public connection details only, never credentials.
    assets = Path(".dev/android-assets")
    assets.mkdir(exist_ok=True)
    debug_profile = {"address": f"{networks[0]['my_hostname']}.{networks[0]['name']}.ray:4433",
                     "hostname": "localhost", "ca": cert}
    (assets / "testing-profile.json").write_text(json.dumps(debug_profile, indent=2))

subprocess.run(["cargo", "build", "-q", "-p", "neobeep-server", "--bins"], check=True)
children = []
try:
    with open(".dev/android-model.log", "a") as model_log, open(".dev/android-server.log", "a") as server_log:
        if not openrouter_key:
            children.append(subprocess.Popen(["python3", "scripts/test-model.py"], stdout=model_log, stderr=model_log))
        env = dict(os.environ, NEOBEEP_TOKEN=token)
        if openrouter_key:
            env["OPENROUTER_API_KEY"] = openrouter_key
        if firecrawl_key:
            env["FIRECRAWL_API_KEY"] = firecrawl_key
        children.append(subprocess.Popen(["target/debug/neobeep-server", ".dev/android-server.toml"], env=env, stdout=server_log, stderr=server_log))
        print(("OpenRouter" if openrouter_key else "TEST provider") + " and server started. Profiles are private files in .dev/.", flush=True)
        if ray_address:
            print(f"Rayfish endpoint: [{ray_address}]:4433", flush=True)
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
