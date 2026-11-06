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
export interface DetailLike {
  run: ListedRun;
}

/// How often a running run opened, and the list while a run goes, read again.
export const POLL_MS = 5_000;
/// The newest runs whose details are read once the list loads.
export const AHEAD = 3;
/// Details read ahead at once.
export const AHEAD_AT_ONCE = 2;
/// Details kept, over every project.
export const MAX_DETAILS = 40;
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

/// Whether the list should be asked again on a poll: a run goes and the
/// rate limit does not hold.
export function listDue(runs: readonly Pick<ListedRun, "state">[], limitedUntil: number | null, now: number) {
  return runs.some(r => going(r.state)) && !limited(limitedUntil, now);
}

/// The rows drawn last, by key, with the markup each was drawn from. A row
/// drawn again from the same markup stays the element on screen, so it
/// does not flash, a spinner keeps turning and nothing below it moves.
export class KeptRows<T> {
  private rows = new Map<string, { sig: string; el: T }>();
  private used = new Set<string>();

  /// The element drawn before for `key` when `sig` is the same, else `fresh`,
  /// which is kept for the next draw.
  take(key: string, sig: string, fresh: T): T {
    this.used.add(key);
    const was = this.rows.get(key);
    if (was && was.sig === sig) return was.el;
    this.rows.set(key, { sig, el: fresh });
    return fresh;
  }

  /// Forgets the rows `prefix` names that were not taken since the last sweep.
  sweep(prefix = "") {
    for (const key of [...this.rows.keys()]) if (key.startsWith(prefix) && !this.used.has(key)) this.rows.delete(key);
    for (const key of [...this.used]) if (key.startsWith(prefix)) this.used.delete(key);
  }

  get size() {
    return this.rows.size;
  }
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
