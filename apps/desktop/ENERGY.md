# Desktop battery profiling

Resource samples go to the existing `node/metrics.log` in the app data directory.
Snapshots are written once a minute and retained for seven days. Phones do not
run the collector. No network listener or telemetry upload is added.

Metrics use the `beep_desktop_` prefix:

- `cpu_milliseconds_total`: cumulative sampled CPU time. Divide the delta by
  elapsed milliseconds for average CPU cores used (1.0 means one busy core).
- `memory_bytes`, `processes`: current resident memory and process count.
- `disk_read_bytes_total`, `disk_written_bytes_total`: cumulative sampled I/O.
  Windows counts all process I/O, not just disk I/O.
- `device_battery_discharge_watts`, `device_battery_energy_joules`: whole-device
  battery measurements, never app-attributed power. Charging discharge is zero.
  Use only when `battery_available` is 1; otherwise old battery gauges are stale.
- `resource_sample_seconds`: collector wall time, to watch its own overhead.

Process roles are `native` (including the embedded node), `webview` (recognized
webview descendants), and `workload` (other descendants, including agents).
Counters use bounded `context` labels (`foreground`, `visible`, `hidden`) and
`power` labels (`battery`, `external`, `unknown`). Context is sampled, not a
continuous activity trace. Intervals whose endpoint labels differ are skipped.
No process IDs, names, arguments, paths, or battery identities are recorded.

The first sample is a baseline. New processes start a baseline; exited processes
and processes that live entirely between samples are missed. PID reuse is
checked against process start time. Gaps over two minutes start a new baseline.
OS-owned or reparented webviews (particularly macOS WebKit XPC services) and
reparented agents can fall outside the process tree. A zero webview count means
no descendants were observed, not zero webview energy. This collector does not
measure GPU power, hardware energy per app, or timer wakeups. Existing transport
metrics cover mesh traffic, not all process network traffic.

## Compare a change

Use the same laptop, brightness, power mode, network, app version, project, and
workload. Let startup finish, then record at least ten minutes per condition:
visible and idle, hidden or minimized and idle, terminal streaming, agent
streaming. Keep other applications quiet. Repeat before and after the change.
Compare CPU and I/O counter deltas, memory, transport traffic, and whole-device
battery discharge. Battery readings alone cannot attribute an improvement to
this app. Use the OS profiler for wakeups, GPU, and missing webview processes.
Record suspend, power-source changes, and sampling gaps; exclude those intervals.
Do not set regression budgets until repeated baselines establish normal variance.

From `apps/desktop/src-tauri`, measure the collector itself with:

```sh
cargo test -p beep-desktop platform::energy::tests::sampling_cost -- --ignored --nocapture
```

This reports wall time, not energy. Compare idle behavior with the collector
enabled and with `BEEP_DESKTOP_RESOURCE_METRICS=0` at app startup to check its CPU/wakeup cost.
