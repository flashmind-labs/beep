"""Lists, downloads and clears the problem reports stored in R2.

    just reports              # newest first
    just report R-ABC...      # download and unzip to target/reports/<id>
    just report-solved <id>   # delete the zip and its manifest

Reads the bucket and keys from `[reports.r2]` in the server config given with
--config. Objects are `reports/<mesh>/<id>.zip` and `<id>.json`.
"""

import argparse
import json
import re
import sys
import tomllib
import zipfile
from datetime import datetime, timezone
from pathlib import Path

import boto3

ID = re.compile(r"[a-z2-7]{16}")


def client(config_path):
    try:
        with open(config_path, "rb") as f:
            r2 = tomllib.load(f).get("reports", {}).get("r2")
    except (OSError, tomllib.TOMLDecodeError) as e:
        sys.exit(f"Cannot read {config_path}: {e}")
    if not r2:
        sys.exit(f"No [reports.r2] in {config_path}")
    s3 = boto3.client(
        "s3",
        endpoint_url=f"https://{r2['account_id']}.r2.cloudflarestorage.com",
        region_name="auto",
        aws_access_key_id=r2["access_key_id"],
        aws_secret_access_key=r2["secret_access_key"],
    )
    return s3, r2["bucket"]


def report_id(text):
    rid = text.strip().lower().removeprefix("r-")
    if not ID.fullmatch(rid):
        sys.exit(f"Not a report reference: {text}")
    return rid


def objects(s3, bucket):
    for page in s3.get_paginator("list_objects_v2").paginate(Bucket=bucket, Prefix="reports/"):
        yield from page.get("Contents", [])


def keys(s3, bucket, rid):
    """The zip and json keys of a report, found by id under any mesh."""
    found = [
        o["Key"]
        for o in objects(s3, bucket)
        if o["Key"].endswith((f"/{rid}.zip", f"/{rid}.json"))
    ]
    if not found:
        sys.exit(f"No report {rid}")
    return found


def list_reports(s3, bucket):
    rows = []
    for obj in objects(s3, bucket):
        if obj["Key"].endswith(".json"):
            try:
                body = s3.get_object(Bucket=bucket, Key=obj["Key"])["Body"].read()
                m = json.loads(body)
                m["made_at"], m["text"], m["machines"]
            except (ValueError, KeyError, TypeError) as e:
                print(f"Skipping {obj['Key']}: {e!r}", file=sys.stderr)
                continue
            rows.append((m, obj["Key"]))
    for m, key in sorted(rows, key=lambda r: r[0]["made_at"], reverse=True):
        mesh = key.split("/")[1][:8]
        ref = "R-" + key.rsplit("/", 1)[1].removesuffix(".json").upper()
        day = datetime.fromtimestamp(m["made_at"], timezone.utc).strftime("%Y-%m-%d")
        text = " ".join(printable(m["text"]).split())[:60]
        machines = m["machines"]
        answered = sum(1 for x in machines if x["answer"] in ("Answered", "OlderNode"))
        print(f"{mesh}  {ref}  {day}  {text}  {answered}/{len(machines)}")


def printable(s):
    """The text without control characters a terminal would act on."""
    return "".join(c for c in s if c.isprintable() or c == "\n")


def unsafe(name):
    path = Path(name)
    return name.startswith(("/", "\\")) or ":" in name.split("/")[0] or path.is_absolute() or ".." in path.parts or ".." in name.split("\\")


def get(s3, bucket, rid):
    key = next((k for k in keys(s3, bucket, rid) if k.endswith(".zip")), None)
    if key is None:
        sys.exit(f"No zip for {rid}")
    out = Path("target/reports")
    out.mkdir(parents=True, exist_ok=True)
    zip_path = out / f"{rid}.zip"
    s3.download_file(bucket, key, str(zip_path))
    dest = out / rid
    dest.mkdir(exist_ok=True)
    with zipfile.ZipFile(zip_path) as z:
        for name in z.namelist():
            if unsafe(name):
                sys.exit(f"Refusing unsafe name in the zip: {name}")
        z.extractall(dest)
    print(zip_path)
    print(dest)


def solved(s3, bucket, rid):
    found = keys(s3, bucket, rid)
    failed = []
    for key in found:
        try:
            s3.delete_object(Bucket=bucket, Key=key)
        except Exception as e:
            failed.append(f"{key}: {e}")
    if failed:
        sys.exit("Delete failed:\n" + "\n".join(failed))
    print(f"Deleted {len(found)} objects of {rid}")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--config", required=True)
    p.add_argument("command", choices=["list", "get", "solved"])
    p.add_argument("id", nargs="?")
    args = p.parse_args()
    if args.command != "list" and not args.id:
        p.error(f"{args.command} needs a report id")
    s3, bucket = client(args.config)
    if args.command == "list":
        list_reports(s3, bucket)
    elif args.command == "get":
        get(s3, bucket, report_id(args.id))
    else:
        solved(s3, bucket, report_id(args.id))


if __name__ == "__main__":
    main()
