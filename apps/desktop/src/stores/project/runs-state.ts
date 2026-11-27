import { plural } from "../../ui/format.ts";
import type { HostRow, RunProgress, RunRow, RunState, RunsRow } from "../../parts/project/runs.tsx";

// What the Actions section keeps and when it reads again, with no page
// behind it so the tests load it: a run's details kept per project and run,
// when one is read again, which runs are read ahead, and which drawn rows
// stay on screen as they are.

/// A run as the list gives it, as far as this file looks at it.
export interface ListedRun {
  id: number;
  host?: string | null;
  state: string;
  finished_at: number | null;
}

/// A run's details as far as this file looks at them.
interface DetailLike {
  run: ListedRun;
}

/// How often a running run opened, and the list while a run goes, read again.
export const POLL_MS = 5_000;
/// The newest runs whose details are read once the list loads.
const AHEAD = 3;
/// Details read ahead at once.
const AHEAD_AT_ONCE = 2;
/// Details kept, over every project.
const MAX_DETAILS = 40;
/// How long the pointer rests on a run before its details are read ahead.
export const HOVER_MS = 150;

/// Not done yet.
export function going(state: string) {
  return state === "queued" || state === "waiting" || state === "running";
}

/// Which run a row is: its host and id, as ids repeat across hosts.
export function runKey(r: Pick<ListedRun, "id" | "host">) {
  return `${r.host ?? ""}#${r.id}`;
}

/// A run of a project, as the kept details are keyed.
export function detailKey(project: string, r: Pick<ListedRun, "id" | "host">) {
  return `${project}\n${runKey(r)}`;
}

/// One read kept, and when it was read.
export interface Kept<T> {
  data: T;
  at: number;
}

/// The details read this session, the most recently used last; the oldest
/// go once there are more than `max`.
export class DetailCache<T> {
  private kept = new Map<string, Kept<T>>();
  private max: number;
  constructor(max = MAX_DETAILS) { this.max = max; }

  get(key: string) {
    const k = this.kept.get(key);
    if (k) { this.kept.delete(key); this.kept.set(key, k); }
    return k;
  }

  has(key: string) {
    return this.kept.has(key);
  }

  put(key: string, data: T, at: number) {
    this.kept.delete(key);
    this.kept.set(key, { data, at });
    while (this.kept.size > this.max) this.kept.delete(this.kept.keys().next().value!);
  }

  get size() {
    return this.kept.size;
  }
}

/// Whether the rate limit holds at `now`.
export function limited(until: number | null | undefined, now: number) {
  return !!until && until > now;
}

/// What to do about a run's details: nothing, read them, or read them past
/// what the app keeps (`refresh`). None kept is read. A run the list shows
/// changed since (a re-run, or one that finished) is read past the app's
/// copy. A run still going is read again once `POLL_MS` passed, unless the
/// rate limit holds. A finished run is never read again on its own.
export function detailDue(
  kept: Kept<DetailLike> | undefined,
  listed: ListedRun | undefined,
  now: number,
  limitedUntil: number | null = null,
): { refresh: boolean } | null {
  if (!kept) return { refresh: false };
  const run = kept.data.run;
  if (listed && (listed.state !== run.state || listed.finished_at !== run.finished_at)) return { refresh: true };
  if (going(run.state) && now - kept.at >= POLL_MS && !limited(limitedUntil, now)) return { refresh: false };
  return null;
}

/// The runs to read ahead once the list loads: the newest `n`, and the
/// newest failed run, so its failing step's log is there when it opens.
/// Runs already kept or being read are left out.
export function ahead<R extends ListedRun>(runs: readonly R[], skip: (r: R) => boolean, n = AHEAD): R[] {
  const out = runs.slice(0, n);
  const failed = runs.find(r => r.state === "failed");
  if (failed && !out.includes(failed)) out.push(failed);
  return out.filter(r => !skip(r));
}

/// A queue of reads with at most `limit` running; a key already queued or
/// running is not added again.
export class Bounded {
  private waiting: { key: string; read: () => Promise<unknown> }[] = [];
  private running = new Set<string>();
  private limit: number;
  constructor(limit = AHEAD_AT_ONCE) { this.limit = limit; }

  has(key: string) {
    return this.running.has(key) || this.waiting.some(w => w.key === key);
  }

  add(key: string, read: () => Promise<unknown>) {
    if (this.has(key)) return;
    this.waiting.push({ key, read });
    this.next();
  }

  private next() {
    while (this.running.size < this.limit && this.waiting.length) {
      const { key, read } = this.waiting.shift()!;
      this.running.add(key);
      // A read keeps its own error where its row shows it; the queue only moves on.
      read().catch(() => {}).finally(() => { this.running.delete(key); this.next(); });
    }
  }

  get busy() {
    return this.running.size;
  }
}

/// "GitHub", or "Forgejo" for any other host.
export function hostLabel(host: string | null | undefined, hosts: readonly Pick<HostRow, "host" | "kind">[] = []) {
  const known = hosts.find(h => h.host === host);
  if (known) return known.kind === "github" ? "GitHub" : "Forgejo";
  return host === "github.com" ? "GitHub" : "Forgejo";
}

/// The Actions heading's line per host when runs come from more than one:
/// "GitHub: 1 running · 1 failed", or the host's error.
export function hostCounts(d: Pick<RunsRow, "runs" | "hosts">) {
  const hosts = d.hosts ?? [];
  if (hosts.length < 2) return [];
  return hosts.map(h => {
    const name = hostLabel(h.host, hosts);
    if (h.error) return { text: `${name}: ${h.error}`, error: true, host: h.host };
    const count = runsCount(d.runs.filter(r => r.host === h.host));
    const ran = d.runs.filter(r => r.host === h.host).length;
    return { text: `${name}: ${count || (ran ? plural(ran, "run", "runs") : "no runs")}`, error: false, host: h.host };
  });
}

/// "48s", "3m 12s", "1h 4m": how long `ms` milliseconds is.
function span(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

/// "Build, step 4 of 9: Run tests, 3m 12s", or with no steps from the host
/// "Build, job 2 of 3, 1m 05s".
export function progressText(p: RunProgress, now = Date.now()) {
  if (p.step) {
    const at = p.step.started_at ?? p.job_started_at;
    return `${p.job}, step ${p.step.number} of ${p.step.of}: ${p.step.name}${at ? `, ${span(now - at)}` : ""}`;
  }

  const at = p.job_started_at;
  return `${p.job}, job ${Math.min(p.jobs_done + 1, p.jobs)} of ${p.jobs}${at ? `, ${span(now - at)}` : ""}`;
}

/// How long a run took, or has run so far; "" when it never started.
export function runSpan(r: Pick<RunRow, "started_at" | "finished_at">, now = Date.now()) {
  if (!r.started_at) return "";
  return span((r.finished_at ?? now) - r.started_at);
}

/// A state's chip: its class, its word and whether it spins.
export function runChip(state: RunState): { cls: string; text: string; spin: boolean } {
  switch (state) {
    case "running": return { cls: "running", text: "running", spin: true };
    case "queued": return { cls: "", text: "queued", spin: false };
    case "waiting": return { cls: "blocked", text: "waiting", spin: false };
    case "passed": return { cls: "done", text: "passed", spin: false };
    case "failed": return { cls: "blocked", text: "failed", spin: false };
    case "cancelled": return { cls: "", text: "cancelled", spin: false };
    case "skipped": return { cls: "", text: "skipped", spin: false };
    default: return { cls: "", text: state || "unknown", spin: false };
  }
}

/// The Actions heading's count: "1 running · 2 failed".
export function runsCount(runs: readonly Pick<RunRow, "state">[]) {
  const running = runs.filter(r => going(r.state)).length;
  const failed = runs.filter(r => r.state === "failed").length;
  return [running ? `${running} running` : "", failed ? `${failed} failed` : ""].filter(Boolean).join(" · ");
}
