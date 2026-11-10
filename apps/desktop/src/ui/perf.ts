// Records how responsive the UI is while used (#571): input to next paint, long
// tasks and frames, open to first content and cached to fresh, per fixed view.
// Samples stay in memory and go to the app's metrics in one call about once a minute.
import { onPhone } from "./platform.ts";
import { type PerfBatch, type PerfEntry, type PerfKind, type PerfView, PerfRecorder, inputOf } from "./perf-state.ts";

export type { PerfView } from "./perf-state.ts";
export { perfViewOf } from "./perf-state.ts";

let recorder: PerfRecorder | null = null;

/// Sets the page the window shows, starting the recording on the first call.
export function perfView(view: PerfView) {
  if (recorder) recorder.setView(view);
  else recorder = start(view);
}

/// Shows overlay `view` over the page while `on`.
export function perfLayer(view: PerfView, on: boolean) {
  recorder?.layer(view, on);
}

/// Starts timing `view` opening: the returned call, made once its content is in,
/// records the time to the frame that paints it. A window hidden meanwhile records nothing.
export function perfOpen(view: PerfView): () => void {
  const at = performance.now(), shown = !document.hidden;
  return () => afterPaint(() => { if (shown && !document.hidden) record("open", view, performance.now() - at); });
}

/// Records a finished open timing for `view`, measured elsewhere.
export function perfOpened(view: PerfView, ms: number) {
  record("open", view, ms);
}

/// Starts timing `view` from its cached draw: the returned call, made once fresh
/// data is drawn, records the time between.
export function perfFresh(view: PerfView): () => void {
  const at = performance.now();
  let done = false;
  return () => {
    if (done) return;
    done = true;
    record("fresh", view, performance.now() - at);
  };
}

function record(kind: PerfKind, view: PerfView, ms: number) {
  recorder?.record(kind, view, ms);
}

function afterPaint(run: () => void) {
  requestAnimationFrame(() => requestAnimationFrame(run));
}

function start(view: PerfView): PerfRecorder {
  const name = view === "bar" ? "bar" : onPhone() ? "phone" : "main";
  const created = new PerfRecorder(name, view, !document.hidden, {
    now: () => performance.now(),
    later: (run, ms) => window.setTimeout(run, ms),
    cancel: timer => window.clearTimeout(timer),
  }, send);
  created.supported = observe(created);
  document.addEventListener("visibilitychange", () => created.visibility(!document.hidden));
  window.addEventListener("pagehide", () => created.flush());
  return created;
}

function send(batch: PerfBatch) {
  window.__TAURI__.core.invoke("desktop_ui_metrics", { batch }).catch(() => {});
}

/// The entry types this webview reports; WebKit has no long tasks or long animation frames.
function observe(to: PerfRecorder): PerfEntry[] {
  const types = typeof PerformanceObserver === "undefined" ? [] : PerformanceObserver.supportedEntryTypes;
  const supported: PerfEntry[] = [];

  if (types.includes("event")) {
    // An interaction's entries share its id; its slowest one is its input to next paint.
    const seen = new Set<number>();
    watch(to, { type: "event", buffered: true, durationThreshold: 16 }, entries => {
      const slowest = new Map<number, PerformanceEventTiming>();

      for (const entry of entries) {
        if (!(entry instanceof PerformanceEventTiming) || !entry.interactionId || seen.has(entry.interactionId)) continue;
        const had = slowest.get(entry.interactionId);
        if (!had || entry.duration > had.duration) slowest.set(entry.interactionId, entry);
      }

      for (const [id, entry] of slowest) {
        seen.add(id);
        to.record("interaction", to.viewAt(entry.startTime), entry.duration, inputOf(entry.name));
      }

      if (seen.size > 64) seen.clear();
    });
    supported.push("event");
  }

  for (const [type, kind] of [["longtask", "long_task"], ["long-animation-frame", "frame"]] as const) {
    if (!types.includes(type)) continue;
    watch(to, { type, buffered: true }, entries => {
      for (const entry of entries) to.record(kind, to.viewAt(entry.startTime), entry.duration);
    });
    supported.push(type);
  }

  return supported;
}

/// Observes with `options`, counting the time spent in the callback as overhead.
function watch(to: PerfRecorder, options: PerformanceObserverInit & { durationThreshold?: number }, handle: (entries: PerformanceEntryList) => void) {
  new PerformanceObserver(list => {
    const started = performance.now();
    handle(list.getEntries());
    to.aggregate.cost(performance.now() - started);
  }).observe(options);
}
