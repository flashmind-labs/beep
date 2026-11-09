# Desktop resource profiling

Use the macOS sidecar while diagnosing a problem. It does not restart the app,
install a service, open a listener, or upload results. It exits after the capture.

```sh
python3 scripts/profile-desktop.py --duration 90 --label home-idle
python3 scripts/profile-desktop.py --duration 90 --label agent-streaming --sample-above 50
```

JSON lines contain per-process CPU, memory footprint, wakeups, disk I/O,
QoS CPU time, instructions and cycles. `100%` CPU means one busy core.
WebKit renderers, GPU and networking services are matched to the app's resource
coalition, including services reparented to launchd. Unrelated browsers are
excluded. Descendant agents are reported as workloads. PID reuse starts a fresh
baseline. New processes are discovered every ten seconds; short-lived processes
and final work between observations can be missed.

Stack sampling is opt-in: at most two three-second samples, on one worker.
`--sample-count` changes the cap. Top stack counts include sleeping threads;
they are not CPU percentages. Stripped native binaries cannot identify Rust
functions. Use a symbol-bearing build and Instruments for exact call stacks.
The profiler reports its own CPU cost and subprocess CPU cost separately.

Battery readings describe the whole device. GPU service CPU is not GPU hardware
utilization. Process CPU cannot establish the energy cost of an individual page.
Use `--label` for controlled page comparisons, keeping workload, brightness and
power mode fixed. Do not compare charging power with discharge power.

The sidecar reads bounded tails of the existing local log and `node/metrics.log`.
App metric deltas cover their snapshot timestamps, which can extend before the
capture. Command wait time includes I/O and scheduling; it is not CPU time.
Results stay local and may contain diagnostic symbols; keep captures private.

The in-app process collector is off by default. Set
`BEEP_DESKTOP_RESOURCE_METRICS=1` at startup to enable the legacy minute sampler.
Lightweight page and command timings remain batched in `node/metrics.log`.
Per-command log traces are off by default; set `BEEP_DESKTOP_TRACE_COMMANDS=1`
at startup for receipt/completion traces. The sidecar can summarize these on
older builds too. Routine profiling should use the batched metrics.
