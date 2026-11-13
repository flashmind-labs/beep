#!/usr/bin/env python3
"""On-demand macOS process profiling. JSON lines go to stdout; no app changes needed."""

import argparse
import collections
import ctypes
import json
import math
import os
import pathlib
import plistlib
import re
import resource
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor


class Coalition(ctypes.Structure):
    # Darwin proc_pidcoalitioninfo, including the three reserved words.
    _fields_ = [("ids", ctypes.c_uint64 * 2), ("reserved", ctypes.c_uint64 * 3)]


class Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


class Usage(ctypes.Structure):
    _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [
        (name, ctypes.c_uint64) for name in (
            "user_ns", "system_ns", "idle_wakeups", "interrupt_wakeups", "pageins",
            "wired_bytes", "resident_bytes", "footprint_bytes", "started", "exited",
            "child_user_ns", "child_system_ns", "child_idle_wakeups",
            "child_interrupt_wakeups", "child_pageins", "child_elapsed_ns",
            "read_bytes", "written_bytes", "qos_default_ns", "qos_maintenance_ns",
            "qos_background_ns", "qos_utility_ns", "qos_legacy_ns",
            "qos_user_initiated_ns", "qos_user_interactive_ns", "billed_system_ns",
            "serviced_system_ns", "logical_written_bytes", "max_footprint_bytes",
            "instructions", "cycles", "billed_energy", "serviced_energy",
            "interval_max_footprint_bytes", "runnable_ns",
        )
    ]


COUNTERS = (
    "user_ns", "system_ns", "idle_wakeups", "interrupt_wakeups", "pageins",
    "read_bytes", "written_bytes", "qos_default_ns", "qos_maintenance_ns",
    "qos_background_ns", "qos_utility_ns", "qos_legacy_ns",
    "qos_user_initiated_ns", "qos_user_interactive_ns", "logical_written_bytes",
    "instructions", "cycles", "runnable_ns",
)
GAUGES = ("resident_bytes", "footprint_bytes", "wired_bytes", "max_footprint_bytes")
TRACE = re.compile(r'command trace: (\{.*\})$')
TIMING = re.compile(r'desktop (\w+) ([a-z_\-]+) took ([0-9]+) ms \((\w+)\)')


def emit(kind, **values):
    print(json.dumps({"type": kind, "unix_seconds": time.time(), **values}), flush=True)


class Darwin:
    def __init__(self):
        self.lib = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        self.lib.proc_pidinfo.argtypes = [
            ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int,
        ]
        self.lib.proc_pidinfo.restype = ctypes.c_int
        self.lib.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
        self.lib.proc_pid_rusage.restype = ctypes.c_int
        system = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
        system.mach_timebase_info.argtypes = [ctypes.POINTER(Timebase)]
        base = Timebase()
        if system.mach_timebase_info(ctypes.byref(base)) != 0 or not base.denom:
            raise OSError("cannot read the Mach clock timebase")
        self.timebase = (base.numer, base.denom)

    def coalition(self, pid):
        result = Coalition()
        size = ctypes.sizeof(result)
        if self.lib.proc_pidinfo(pid, 20, 0, ctypes.byref(result), size) != size:
            return None
        return result.ids[0] or None

    def usage(self, pid):
        result = Usage()
        # V4 includes QoS time, instructions and cycles. Older kernels fall back to V2.
        flavor = 4
        if self.lib.proc_pid_rusage(pid, flavor, ctypes.byref(result)) != 0:
            flavor = 2
            if self.lib.proc_pid_rusage(pid, flavor, ctypes.byref(result)) != 0:
                return None
        names = COUNTERS if flavor == 4 else COUNTERS[:7]
        gauges = GAUGES if flavor == 4 else GAUGES[:3]
        values = {name: getattr(result, name) for name in (*names, *gauges, "started")}
        return normalize_times(values, *self.timebase)


def normalize_times(values, numer, denom):
    # rusage CPU and QoS times are Mach ticks, including on Apple Silicon.
    return {name: value * numer // denom if name.endswith("_ns") else value
            for name, value in values.items()}


def processes():
    result = subprocess.run(
        ["/bin/ps", "-axo", "pid=,ppid=,comm="], capture_output=True, text=True,
        check=True, timeout=10,
    )
    rows = {}
    for line in result.stdout.splitlines():
        parts = line.strip().split(None, 2)
        if len(parts) == 3:
            rows[int(parts[0])] = (int(parts[1]), parts[2])
    return rows


def webkit_role(command):
    name = pathlib.Path(command).name
    if name == "com.apple.WebKit.GPU":
        return "gpu_service"
    if name == "com.apple.WebKit.Networking":
        return "network_service"
    if name.startswith("com.apple.WebKit.WebContent"):
        return "renderer"
    return None


def discover(rows, root, coalition, api, excluded=()):
    excluded = set(excluded)
    while True:
        before = len(excluded)
        excluded.update(pid for pid, (parent, _) in rows.items() if parent in excluded)
        if len(excluded) == before:
            break
    selected = {root: "native"}
    if coalition is not None:
        for pid, (_, command) in rows.items():
            role = webkit_role(command)
            if pid not in excluded and role and api.coalition(pid) == coalition:
                selected[pid] = role

    while True:
        before = len(selected)
        for pid, (parent, command) in rows.items():
            if pid not in excluded and pid not in selected and parent in selected:
                selected[pid] = webkit_role(command) or "workload"
        if len(selected) == before:
            return selected


def delta(before, after):
    if before["started"] != after["started"]:
        return None
    return {key: max(0, after[key] - before[key]) for key in COUNTERS if key in before and key in after}


def sample(pid, seconds):
    result = subprocess.run(
        ["/usr/bin/sample", str(pid), str(seconds), "5", "-file", "/dev/stdout"],
        capture_output=True, text=True, timeout=seconds + 20,
    )
    if result.returncode:
        return {"pid": pid, "status": "unavailable", "exit_code": result.returncode}
    # The raw sample can contain private paths. Emit only symbol counts, without image paths.
    text = result.stdout
    start = text.find("Sort by top of stack")
    end = text.find("Binary Images:", start)
    stacks = text[start:end].splitlines()[1:] if start >= 0 else []
    return {"pid": pid, "status": "ok", "top_stacks": [s.strip() for s in stacks if s.strip()][:15]}


def metric_snapshot(path):
    """Read a bounded tail, accepting only the last complete Prometheus snapshot."""
    try:
        with path.open("rb") as handle:
            handle.seek(0, 2)
            handle.seek(max(0, handle.tell() - 2 * 1024 * 1024))
            text = handle.read().decode(errors="replace")
    except OSError:
        return None
    snapshots = text.split("# end snapshot")
    for snapshot in reversed(snapshots[:-1]):
        stamp = re.search(r"# snapshot unix_seconds=(\d+)", snapshot)
        if stamp is None:
            continue
        commands = {}
        pages = {}
        for line in snapshot.splitlines():
            match = re.fullmatch(r'beep_desktop_duration_seconds_(count|sum)\{([^}]*)\} ([0-9.eE+\-]+)', line)
            if match is None:
                continue
            field, labels, raw = match.groups()
            labels = dict(re.findall(r'(\w+)="([a-z_\-]+)"', labels))
            name = labels.get("name")
            if not name:
                continue
            target = commands if labels.get("kind") == "command" else pages if labels.get("kind") == "page_mount_painted" else None
            if target is None:
                continue
            value = float(raw)
            if math.isfinite(value) and value >= 0:
                fields = target.setdefault(name, collections.Counter())
                fields[field] += value
        return {"unix_seconds": int(stamp[1]), "commands": commands, "page_mounts": pages}
    return None


def metric_changes(before, after):
    if before is None or after is None or after["unix_seconds"] <= before["unix_seconds"]:
        return {"available": False}
    result = {"available": True, "seconds": after["unix_seconds"] - before["unix_seconds"]}
    for kind in ("commands", "page_mounts"):
        values = {}
        for name, fields in after[kind].items():
            previous = before[kind].get(name, {})
            if any(value < previous.get(key, 0) for key, value in fields.items()):
                return {"available": False, "reason": "counters reset"}
            changes = {key: value - previous.get(key, 0) for key, value in fields.items()}
            if changes.get("count", 0):
                values[name] = {"calls": changes.get("count", 0), "wait_seconds": changes.get("sum", 0)}
        result[kind] = values
    return result


class LogReader:
    def __init__(self, path):
        self.path = path
        self.handle = None
        self.partial = ""
        self.calls = collections.Counter()
        self.wait_ms = collections.Counter()
        self.slow = collections.Counter()
        self.pages = collections.Counter()
        self.read(first=True)

    def read(self, first=False):
        try:
            stat = self.path.stat()
            if self.handle is not None:
                old = os.fstat(self.handle.fileno())
                if (old.st_dev, old.st_ino) != (stat.st_dev, stat.st_ino) or stat.st_size < self.handle.tell():
                    self.handle.close()
                    self.handle = None
                    self.partial = ""
            if self.handle is None:
                self.handle = self.path.open(errors="replace")
                if first:
                    self.handle.seek(0, 2)
            text = self.partial + self.handle.read(1024 * 1024)
        except OSError:
            return

        lines = text.split("\n")
        self.partial = lines.pop()[-8000:]
        for line in lines:
            match = TRACE.search(line)
            if match:
                try:
                    trace = json.loads(match[1])
                except ValueError:
                    continue
                name = trace.get("command")
                if not isinstance(name, str) or not re.fullmatch(r"[a-z_]{1,80}", name):
                    continue
                if trace.get("stage") == "requested":
                    self.calls[name] += 1
                elif trace.get("stage") == "returned" and isinstance(trace.get("ms"), (int, float)):
                    ms = trace["ms"]
                    if math.isfinite(ms) and 0 <= ms <= 600_000:
                        self.wait_ms[name] += ms
            match = TIMING.search(line)
            if match:
                kind, name, _, _ = match.groups()
                if kind == "page_mount_painted":
                    self.pages[name] += 1
                else:
                    self.slow[name] += 1

    def close(self):
        if self.handle:
            self.handle.close()


def battery():
    try:
        result = subprocess.run(
            ["/usr/sbin/ioreg", "-r", "-c", "AppleSmartBattery", "-a"],
            capture_output=True, timeout=10, check=True,
        )
        rows = plistlib.loads(result.stdout)
        row = next(iter(rows), {})
        voltage, amperage = row.get("Voltage"), row.get("Amperage")
        if not isinstance(voltage, int) or not isinstance(amperage, int):
            return {"available": False}
        if amperage >= 2**63:
            amperage -= 2**64
        if not 0 < voltage < 100_000 or abs(amperage) > 100_000:
            return {"available": False}
        return {"available": True, "external": row.get("ExternalConnected"),
                "charging": row.get("IsCharging"), "voltage_volts": voltage / 1000,
                "current_amps": amperage / 1000,
                "battery_discharge_watts": max(0, -voltage * amperage / 1_000_000)}
    except (OSError, ValueError, subprocess.SubprocessError, plistlib.InvalidFileException):
        return {"available": False}


def positive(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("must be a finite positive number")
    return number


def default_log():
    if sys.platform == "darwin":
        return pathlib.Path.home() / "Library/Logs/Beep/beep.log"
    # Matches dirs::state_dir() on Linux: an absolute XDG_STATE_HOME, else ~/.local/state.
    state = os.environ.get("XDG_STATE_HOME", "")
    base = pathlib.Path(state) if os.path.isabs(state) else pathlib.Path.home() / ".local/state"
    return base / "beep/logs/beep.log"


def main():
    profiler_started = time.monotonic()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, help="desktop app PID, otherwise discover the running app")
    parser.add_argument("--duration", type=positive, default=60)
    parser.add_argument("--interval", type=positive, default=2)
    parser.add_argument("--label", default="unspecified", help="test condition, such as home-idle")
    parser.add_argument("--sample-above", type=positive, help="opt in to stack sampling above this CPU percent")
    parser.add_argument("--sample-count", type=int, choices=range(1, 11), default=2)
    parser.add_argument("--log", type=pathlib.Path, default=default_log())
    parser.add_argument("--metrics", type=pathlib.Path,
                        default=pathlib.Path.home() / "Library/Application Support/com.flashmind.beep.desktop/node/metrics.log")
    parser.add_argument("--no-battery", action="store_true")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("this profiler uses macOS libproc; run it on the Mac")
    if args.interval < 0.25:
        parser.error("--interval must be at least 0.25 seconds")

    api = Darwin()
    rows = processes()
    roots = [pid for pid, (parent, name) in rows.items()
             if pathlib.Path(name).name == "beep-desktop" and parent == 1]
    if args.pid is None and len(roots) != 1:
        parser.error("could not identify one desktop process; pass --pid")
    root = args.pid if args.pid is not None else roots[0]
    identity = api.usage(root)
    if identity is None:
        parser.error("cannot read the app's resource counters")
    coalition = api.coalition(root)
    selected = discover(rows, root, coalition, api, excluded=(os.getpid(),))
    log = LogReader(args.log)
    metrics_before = metric_snapshot(args.metrics)
    previous = {}
    totals = {}
    futures = {}
    start = previous_at = time.monotonic()
    next_discovery = start + 10
    next_battery = start
    pool = ThreadPoolExecutor(max_workers=1)
    emit("start", label=args.label, pid=root, coalition_available=coalition is not None,
         processes=selected, interval_seconds=args.interval,
         stack_sampling=args.sample_above is not None)
    for pid in selected:
        got = api.usage(pid)
        if got:
            previous[pid] = got

    try:
        while time.monotonic() - start < args.duration:
            time.sleep(min(args.interval, max(0, args.duration - (time.monotonic() - start))))
            now = time.monotonic()
            root_usage = api.usage(root)
            if root_usage is None or root_usage["started"] != identity["started"]:
                emit("stopped", reason="app exited or restarted")
                break
            if now >= next_discovery:
                selected = discover(processes(), root, coalition, api, excluded=(os.getpid(),))
                next_discovery = now + 10

            current = {}
            for pid, role in selected.items():
                got = api.usage(pid)
                if got is None:
                    continue
                current[pid] = got
                before = previous.get(pid)
                change = delta(before, got) if before else None
                if change is None:
                    continue
                elapsed = now - previous_at
                cpu = (change["user_ns"] + change["system_ns"]) / elapsed / 1e7
                key = (pid, got["started"])
                total = totals.setdefault(key, {"role": role, "seconds": 0, "peak_cpu_percent": 0,
                                                "counters": collections.Counter()})
                total["seconds"] += elapsed
                total["peak_cpu_percent"] = max(total["peak_cpu_percent"], cpu)
                total["counters"].update(change)
                total["memory"] = {key: got[key] for key in GAUGES if key in got}
                emit("process", label=args.label, pid=pid, role=role, seconds=elapsed,
                     cpu_percent=round(cpu, 2), counters=change, memory=total["memory"])
                if (args.sample_above is not None and cpu >= args.sample_above
                        and key not in futures and len(futures) < args.sample_count):
                    futures[key] = pool.submit(sample, pid, 3)
            previous, previous_at = current, now
            log.read()
            if not args.no_battery and now >= next_battery:
                emit("device_battery", **battery())
                next_battery = now + 60
    except KeyboardInterrupt:
        pass
    finally:
        log.read()
        log.close()
        for (pid, _), total in totals.items():
            counts, seconds = total["counters"], total["seconds"]
            emit("summary", label=args.label, pid=pid, role=total["role"], seconds=seconds,
                 avg_cpu_percent=round((counts["user_ns"] + counts["system_ns"]) / seconds / 1e7, 2),
                 peak_cpu_percent=round(total["peak_cpu_percent"], 2), counters=counts, memory=total["memory"])
        emit("commands", calls=log.calls, wait_ms=log.wait_ms, slow_calls=log.slow, page_mounts=log.pages)
        emit("app_metrics", **metric_changes(metrics_before, metric_snapshot(args.metrics)))
        for future in futures.values():
            try:
                emit("stack_sample", **future.result())
            except (OSError, subprocess.SubprocessError):
                emit("stack_sample", status="unavailable")
        pool.shutdown(wait=True)
        children_end = resource.getrusage(resource.RUSAGE_CHILDREN)
        emit("profiler", elapsed_seconds=time.monotonic() - profiler_started,
             capture_seconds=time.monotonic() - start, cpu_seconds=time.process_time(),
             subprocess_cpu_seconds=children_end.ru_utime + children_end.ru_stime)


if __name__ == "__main__":
    main()
