// UI timings kept in memory as fixed-bucket histograms until the next flush (#571).
// Every sample carries only a fixed view name and kind: never content, ids, paths or names.

export const PERF_VIEWS = ["home", "session", "new", "project", "diff", "picker", "settings", "bar", "terminals", "list", "other"] as const;
export type PerfView = typeof PERF_VIEWS[number];
export type PerfWindow = "main" | "bar" | "phone";
/// What a sample measures: input to next paint, a long task, a long animation frame,
/// open to first content, or a cached draw to fresh data.
export type PerfKind = "interaction" | "long_task" | "frame" | "open" | "fresh";
export type PerfInput = "pointer" | "key" | "other";
export type PerfEntry = "event" | "longtask" | "long-animation-frame";

/// Upper bounds in ms; one more bucket holds everything above the last.
export const PERF_BUCKETS_MS = [16, 33, 50, 100, 200, 300, 500, 1000, 2000, 5000] as const;
/// One slow sample each batch carries for the app's log.
export const SLOW_MS: Partial<Record<PerfKind, number>> = { interaction: 500, long_task: 200 };
const MAX_SLOW = 8;
const MAX_MS = 600_000;

export interface PerfHistogram {
  kind: PerfKind;
  view: PerfView;
  input: PerfInput | null;
  counts: number[];
  sum_ms: number;
  max_ms: number;
}

export interface PerfSlow { kind: PerfKind; view: PerfView; ms: number }

/// One flush, as `desktop_ui_metrics` takes it.
export interface PerfBatch {
  window: PerfWindow;
  histograms: PerfHistogram[];
  shown_ms: { view: PerfView; ms: number }[];
  hidden_ms: number;
  slow: PerfSlow[];
  supported: PerfEntry[];
  overhead_ms: number;
}

/// Which view a page of the window is, from its page name in `page-timing.ts`.
export function perfViewOf(page: string): PerfView {
  switch (page) {
    case "home": return "home";
    case "agent": return "session";
    case "agentnew": return "new";
    case "projects": return "project";
    case "terminals": return "terminals";
    case "settings": return "settings";
    case "agents": return "list";
    default: return "other";
  }
}

/// The input an Event Timing entry's event type belongs to.
export function inputOf(event: string): PerfInput {
  if (event.startsWith("key")) return "key";
  if (/^(?:pointer|mouse|touch|click|dblclick|contextmenu|auxclick)/.test(event)) return "pointer";
  return "other";
}

export function bucketOf(ms: number): number {
  const index = PERF_BUCKETS_MS.findIndex(bound => ms <= bound);
  return index === -1 ? PERF_BUCKETS_MS.length : index;
}

/// The samples and shown time since the last flush.
export class PerfAggregate {
  private histograms = new Map<string, PerfHistogram>();
  private shown = new Map<PerfView, number>();
  private hidden = 0;
  private slow: PerfSlow[] = [];
  private overhead = 0;

  record(kind: PerfKind, view: PerfView, ms: number, input: PerfInput | null = null) {
    if (!Number.isFinite(ms) || ms < 0 || ms > MAX_MS) return;
    const key = `${kind} ${view} ${input ?? ""}`;
    let histogram = this.histograms.get(key);

    if (!histogram) {
      histogram = { kind, view, input, counts: new Array<number>(PERF_BUCKETS_MS.length + 1).fill(0), sum_ms: 0, max_ms: 0 };
      this.histograms.set(key, histogram);
    }

    histogram.counts[bucketOf(ms)]++;
    histogram.sum_ms += ms;
    histogram.max_ms = Math.max(histogram.max_ms, ms);
    const slow = SLOW_MS[kind];
    if (slow !== undefined && ms > slow && this.slow.length < MAX_SLOW) this.slow.push({ kind, view, ms: Math.round(ms) });
  }

  dwell(view: PerfView, shown: boolean, ms: number) {
    if (!(ms > 0)) return;
    if (shown) this.shown.set(view, (this.shown.get(view) ?? 0) + ms);
    else this.hidden += ms;
  }

  cost(ms: number) { this.overhead += ms; }

  /// The batch to send, emptying the aggregate; null when nothing is pending. Hidden time
  /// alone waits for samples or shown time; with `samplesOnly`, as on the timer, shown time
  /// waits too, so an idle window sends nothing.
  take(window: PerfWindow, supported: PerfEntry[], samplesOnly = false): PerfBatch | null {
    if (!this.histograms.size && (samplesOnly || !this.shown.size)) return null;
    const batch: PerfBatch = {
      window,
      histograms: [...this.histograms.values()],
      shown_ms: [...this.shown].map(([view, ms]) => ({ view, ms: Math.round(ms) })),
      hidden_ms: Math.round(this.hidden),
      slow: this.slow,
      supported,
      overhead_ms: this.overhead,
    };
    this.histograms = new Map();
    this.shown = new Map();
    this.hidden = 0;
    this.slow = [];
    this.overhead = 0;
    return batch;
  }
}

/// The view shown at a moment shortly before now, so an input is charged to
/// the view it happened in even when it opened another.
export class ViewHistory {
  private changes: { at: number; view: PerfView }[];

  constructor(view: PerfView, at: number) { this.changes = [{ at, view }]; }

  get current(): PerfView { return this.changes[this.changes.length - 1].view; }

  set(view: PerfView, at: number) {
    if (view === this.current) return;
    this.changes.push({ at, view });
    if (this.changes.length > 16) this.changes.shift();
  }

  at(time: number): PerfView {
    for (let i = this.changes.length - 1; i >= 0; i--) if (this.changes[i].at <= time) return this.changes[i].view;
    return this.changes[0].view;
  }
}

/// About how often a shown window sends its samples.
export const PERF_FLUSH_MS = 60_000;
/// A slow sample is sent this soon, at most once per `PERF_FLUSH_MS`, so its log line is near it in time.
export const PERF_SLOW_FLUSH_MS = 2_000;

export interface PerfClock {
  now(): number;
  later(run: () => void, ms: number): number;
  cancel(timer: number): void;
}

/// Owns the aggregate, the view shown and the flush timer. The timer runs only
/// while the window shows; hiding sends once and leaves no timer behind.
export class PerfRecorder {
  readonly aggregate = new PerfAggregate();
  private readonly views: ViewHistory;
  private readonly layers: PerfView[] = [];
  private base: PerfView;
  private shown: boolean;
  private since: number;
  private timer: number | null = null;
  private lastSlowFlush = -Infinity;
  private readonly window: PerfWindow;
  private readonly clock: PerfClock;
  private readonly send: (batch: PerfBatch) => void;
  supported: PerfEntry[] = [];

  constructor(window: PerfWindow, view: PerfView, shown: boolean, clock: PerfClock, send: (batch: PerfBatch) => void) {
    this.window = window;
    this.clock = clock;
    this.send = send;
    this.base = view;
    this.shown = shown;
    this.since = clock.now();
    this.views = new ViewHistory(view, this.since);
    if (shown) this.arm(PERF_FLUSH_MS);
  }

  get view(): PerfView { return this.views.current; }

  viewAt(time: number): PerfView { return this.views.at(time); }

  /// The page under any open layer.
  setView(view: PerfView) {
    this.base = view;
    this.changeView();
  }

  /// An overlay such as Settings or the model picker, shown over the page while `on`.
  layer(view: PerfView, on: boolean) {
    const at = this.layers.lastIndexOf(view);
    if (on) this.layers.push(view);
    else if (at !== -1) this.layers.splice(at, 1);
    this.changeView();
  }

  record(kind: PerfKind, view: PerfView, ms: number, input: PerfInput | null = null) {
    this.aggregate.record(kind, view, ms, input);
    const slow = SLOW_MS[kind];
    if (!this.shown || slow === undefined || !(ms > slow)) return;
    const now = this.clock.now();

    if (now - this.lastSlowFlush >= PERF_FLUSH_MS) {
      this.lastSlowFlush = now;
      this.arm(PERF_SLOW_FLUSH_MS);
    }
  }

  visibility(shown: boolean) {
    if (shown === this.shown) return;
    this.settle();
    this.shown = shown;

    if (shown) this.arm(PERF_FLUSH_MS);
    else this.flush();
  }

  /// Sends what is pending on hide and as the page goes.
  flush() { this.sendPending(false); }

  /// The timer sends only samples; shown time joins them, or goes on hide.
  private sendPending(samplesOnly: boolean) {
    const started = this.clock.now();
    if (this.timer !== null) this.clock.cancel(this.timer);
    this.timer = null;
    this.settle();
    const batch = this.aggregate.take(this.window, this.supported, samplesOnly);
    if (batch) this.send(batch);
    // Building and sending this batch is counted in the next one.
    this.aggregate.cost(this.clock.now() - started);
    if (this.shown) this.arm(PERF_FLUSH_MS);
  }

  private changeView() {
    const view = this.layers[this.layers.length - 1] ?? this.base;
    if (view === this.views.current) return;
    this.settle();
    this.views.set(view, this.clock.now());
  }

  /// Charges the time since the last change to the view shown, or to hidden time.
  private settle() {
    const now = this.clock.now();
    this.aggregate.dwell(this.views.current, this.shown, now - this.since);
    this.since = now;
  }

  /// Moves the next flush to `ms` from now when that is sooner.
  private arm(ms: number) {
    if (this.timer !== null) {
      if (ms >= PERF_FLUSH_MS) return;
      this.clock.cancel(this.timer);
    }

    this.timer = this.clock.later(() => { this.timer = null; this.sendPending(true); }, ms);
  }
}
