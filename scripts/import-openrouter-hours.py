#!/usr/bin/env python3
"""Import complete UTC billing hours for a flsh session on the machine that ran it.

Input is JSONL of OpenRouter /generation data objects for the entire interval.
Use the provider session ID verified against the local transcript. The range is
inclusive at the start, exclusive at the end. Keep the two sides of a move on
their respective machines.
"""

import argparse
import datetime
import hashlib
import json
import os
import tempfile
from decimal import Decimal
from pathlib import Path


def utc_hour(value):
    date = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if date.utcoffset() != datetime.timedelta(0) or date.minute or date.second or date.microsecond:
        raise ValueError("expected a whole UTC hour")
    return int(date.timestamp())


def billed_hours(records, session, start, end):
    hours = {}
    seen = {}
    for record in records:
        if record["session_id"] != session:
            continue
        timestamp = datetime.datetime.fromisoformat(record["created_at"].replace("Z", "+00:00"))
        if timestamp.utcoffset() is None:
            raise ValueError("generation timestamp has no time zone")
        hour = int(timestamp.timestamp()) // 3600 * 3600
        if not start <= hour < end:
            continue
        generation = record["id"]
        if generation in seen:
            if seen[generation] != record:
                raise ValueError("generation has conflicting billing records")
            continue
        seen[generation] = record
        model = record["model"]
        usd = Decimal(str(record["total_cost"]))
        counts = [record[k] for k in ("native_tokens_prompt", "native_tokens_completion")]
        cached = record["native_tokens_cached"] or 0
        if not model or not usd.is_finite() or usd < 0 or any(
            type(n) is not int or n < 0 for n in [*counts, cached]
        ) or cached > counts[0]:
            raise ValueError("invalid generation totals")
        totals = hours.setdefault(str(hour), {}).setdefault(
            model, {"input": 0, "cached": 0, "output": 0, "usd": Decimal(0)}
        )
        totals["input"] += counts[0]
        totals["output"] += counts[1]
        totals["cached"] += cached
        totals["usd"] += usd

    if not hours:
        raise ValueError("no matching billed generations")
    for models in hours.values():
        for totals in models.values():
            totals["usd"] = float(totals["usd"])
    return hours


def store_hours(session_path, hours):
    target = session_path.with_suffix(".openrouter.json")
    # Only importers use this lock. Readers see the old or new file atomically.
    lock = target.with_suffix(".lock")
    try:
        with lock.open("x"):
            pass
    except FileExistsError:
        raise ValueError(f"another import holds {lock}") from None
    try:
        previous = json.loads(target.read_text()) if target.exists() else {}
        for hour, models in hours.items():
            if hour in previous and previous[hour] != models:
                raise ValueError("an imported hour disagrees with the existing billing record")
        combined = previous | hours
        if combined == previous:
            return
        with tempfile.NamedTemporaryFile(dir=target.parent, mode="w", delete=False) as f:
            name = f.name
            try:
                json.dump(combined, f, sort_keys=True)
                f.flush()
                os.fsync(f.fileno())
                f.close()
                os.replace(name, target)
            finally:
                Path(name).unlink(missing_ok=True)
    finally:
        lock.unlink()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("generations", type=Path)
    parser.add_argument("local_session", type=Path)
    parser.add_argument("provider_session")
    parser.add_argument("start", type=utc_hour)
    parser.add_argument("end", type=utc_hour)
    args = parser.parse_args()
    if not args.start < args.end <= datetime.datetime.now(datetime.timezone.utc).timestamp():
        parser.error("select a completed billing interval")
    with args.local_session.open() as f:
        header = json.loads(f.readline())
    if not header.get("sessionId") or not header.get("metadata", {}).get("working_dir"):
        parser.error("expected a flsh session header")
    # Older ACP versions sent a key shared by sessions in the same folder.
    cwd = Path(header["metadata"]["working_dir"]).resolve()
    folder_id = hashlib.sha256(b"flsh:project-cache:v1\0" + os.fsencode(cwd)).hexdigest()
    if args.provider_session != header["sessionId"]:
        if args.provider_session != folder_id:
            parser.error("provider session does not match this session or its folder")
        if any(p != args.local_session for p in args.local_session.parent.glob("*.jsonl")):
            parser.error("the provider folder key is shared by multiple local sessions")
    with args.generations.open() as f:
        hours = billed_hours(map(json.loads, f), args.provider_session, args.start, args.end)
    store_hours(args.local_session, hours)


if __name__ == "__main__":
    main()
