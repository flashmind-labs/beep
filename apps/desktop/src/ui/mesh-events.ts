// Views and plugins share topic subscriptions over the window's duplex node connections.
import { onShownChange, pageShown } from "./active.ts";
import { matchesTopic, normalizeTopic, type Topic } from "./topic-state.ts";
import { clearViews, installViews } from "./snapshot-state.ts";
export { matchesTopic, topic, type Topic } from "./topic-state.ts";

/// `seq` orders the updates native code retains, so a reader that attaches again asks only for newer ones.
export interface MeshEvent { machine_id: string; topic: Topic; revision: number; query?: number | null; initial?: boolean; data?: unknown; seq?: number }
interface Interest { machine: string | null; topic: Topic }
type Receive = (event: MeshEvent) => unknown;
interface Subscriber {
  interests: Interest[];
  receive: Receive;
  active: () => boolean;
  coalesce: boolean;
  pending: Map<string, { event: MeshEvent; bytes: number }>;
  pendingBytes: number;
  running: boolean;
  failures: number;
  /// Milliseconds a coalesced subscriber waits before reading again.
  wait: number;
  timer?: ReturnType<typeof setTimeout>;
}

const subscribers = new Set<Subscriber>();
// While the window is hidden the reader is paused: native code keeps its node streams and the
// newest updates for a grace period and sends only what changed once the window shows again.
interface Reader {
  id: string;
  wanted: Interest[];
  sent: string | null;
  paused: boolean;
  updating: boolean;
  /// The highest update sequence received.
  seq: number;
}
/// Where a reader that ended can attach again, for the same topics only.
interface ResumePoint { seq: number; topics: string }
let reader: Reader | null = null;
let resumeFrom: ResumePoint | null = null;
let retry: ReturnType<typeof setTimeout> | undefined;
let queued = false;

const active = (subscriber: Subscriber) => pageShown() && subscriber.active();

/// Omitting a machine subscribes across the mesh. `*` includes plugin namespaces too.
export function subscribeTopics(topics: Topic | Topic[], receive: Receive, machine: string | null = null, when: () => boolean = () => true): () => void {
  return subscribe(topics, receive, machine, when, false);
}

/// Initial query answers return through the requesting read; coalesce subsequent snapshots.
/// Live views keep the default wait; ambient ones pass `AMBIENT_MS` or `SLOW_MS`.
/// A receiver declared without parameters runs once for all pending updates.
export function subscribeChanges(topics: Topic | Topic[], receive: Receive, machine: string | null = null, when: () => boolean = () => true, wait = 100): () => void {
  return subscribe(topics, receive, machine, when, true, wait);
}

function subscribe(topics: Topic | Topic[], receive: Receive, machine: string | null, when: () => boolean, coalesce: boolean, wait = 100): () => void {
  if (typeof window === "undefined") return () => {};
  const subscriber: Subscriber = {
    interests: (Array.isArray(topics) ? topics : [topics]).map(topic => ({ machine, topic: normalizeTopic(topic) })),
    receive, active: when, coalesce, pending: new Map(), pendingBytes: 0, running: false, failures: 0, wait,
  };
  subscribers.add(subscriber);
  // Module-level subscribers can depend on state in a circular import.
  queueMicrotask(refreshTopicSubscriptions);
  return () => {
    clearTimeout(subscriber.timer);
    subscriber.pending.clear();
    subscriber.pendingBytes = 0;
    subscribers.delete(subscriber);
    refreshTopicSubscriptions();
  };
}

/// A hidden window keeps the interests of its views, so showing it again changes nothing natively.
function interests() {
  const wanted = [...subscribers].filter(subscriber => subscriber.active()).flatMap(subscriber => subscriber.interests);
  return [...new Map(wanted.map(interest => [JSON.stringify(interest), interest])).values()]
    .sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
}

/// Call after changing a route, tab or resource whose owner decides whether it is visible.
export function refreshTopicSubscriptions() {
  for (const subscriber of subscribers) {
    if (!active(subscriber)) {
      clearTimeout(subscriber.timer);
      subscriber.timer = undefined;
    } else if (subscriber.pending.size) {
      schedule(subscriber);
    }
  }

  if (queued) return;
  queued = true;
  queueMicrotask(() => { queued = false; void flush(); });
}

async function flush() {
  const { Channel, invoke } = await import("./tauri.ts");
  const topics = interests();
  if (!topics.length) { stop(); return; }
  if (reader) { reader.wanted = topics; void sendTopics(reader); return; }
  if (!pageShown()) return;
  const key = JSON.stringify(topics);
  const since = resumeFrom?.topics === key ? resumeFrom.seq : null;
  resumeFrom = null;
  if (since === null) clearViews();
  const current: Reader = { id: crypto.randomUUID(), wanted: topics, sent: key, paused: false, updating: false, seq: since ?? 0 };
  reader = current;
  const onEvent = new Channel<MeshEvent>();
  let first = true;
  onEvent.onmessage = event => {
    if (reader !== current) {
      void invoke("mesh_topics", { id: current.id, topics: null }).catch(() => {});
      return;
    }

    if (typeof event.seq === "number" && event.seq > current.seq) current.seq = event.seq;
    if (first) { first = false; current.sent = null; current.paused = false; refreshTopicSubscriptions(); }
    installViews(event.data);

    // An update sent just before a pause is kept; hidden views read it once shown.
    for (const subscriber of subscribers) {
      if (!subscriber.active() || !subscriber.interests.some(interest => (interest.machine === null || interest.machine === event.machine_id)
        && matchesTopic(interest.topic, event.topic))) continue;
      if (subscriber.coalesce && event.initial && event.query != null) continue;

      if (!subscriber.coalesce) {
        try { void Promise.resolve(subscriber.receive(event)).catch(report); } catch (error) { report(error); }
      } else {
        markDirty(subscriber, event);
        schedule(subscriber);
      }
    }
  };
  void invoke("mesh_watch", { id: current.id, topics, onEvent, since }).catch(e => console.warn("mesh stream:", e)).finally(() => {
    if (reader !== current) return;
    reader = null;
    resumeFrom = current.sent === null ? null : { seq: current.seq, topics: current.sent };
    if (interests().length) retryLater();
  });
}

// Serialize changes so a late IPC response cannot restore an older set of interests.
async function sendTopics(current: Reader) {
  if (current.updating) return;
  current.updating = true;

  try {
    const { invoke } = await import("./tauri.ts");

    while (reader === current) {
      const wanted = current.wanted, key = JSON.stringify(wanted), hidden = !pageShown();

      if (hidden) {
        if (current.paused) break;
        await invoke("mesh_topics", { id: current.id, topics: wanted, paused: true });
        current.paused = true;
      } else {
        if (current.sent === key && !current.paused) break;
        await invoke("mesh_topics", current.paused ? { id: current.id, topics: wanted, paused: false } : { id: current.id, topics: wanted });
        current.paused = false;
      }

      current.sent = key;
    }
  } catch (error) {
    console.warn("mesh topics:", error);

    if (reader === current) {
      retryLater();
    }
  } finally {
    current.updating = false;
  }
}

/// A retry runs only while the page exists; a torn-down page (or test) is never revisited.
function retryLater() {
  clearTimeout(retry);
  retry = setTimeout(() => { if (typeof document !== "undefined") refreshTopicSubscriptions(); }, 5000);
}

function report(error: unknown) { console.warn("mesh subscriber:", error); }

const MAX_PENDING = 256;
const MAX_PENDING_BYTES = 8 * 1024 * 1024;
function failed(event: MeshEvent) {
  if (!event.data || typeof event.data !== "object") return false;
  if ("error" in event.data && typeof event.data.error === "string") return true;
  return "views" in event.data && Array.isArray(event.data.views)
    && event.data.views.some((view: unknown) => view !== null && typeof view === "object" && "error" in view && typeof view.error === "string");
}

function markDirty(subscriber: Subscriber, event: MeshEvent) {
  const key = JSON.stringify([event.machine_id, event.topic, event.query ?? null]);
  const previous = subscriber.pending.get(key);
  if (previous) { subscriber.pendingBytes -= previous.bytes; subscriber.pending.delete(key); }
  const bytes = new TextEncoder().encode(JSON.stringify(event)).byteLength;
  subscriber.pending.set(key, { event, bytes });
  subscriber.pendingBytes += bytes;

  while (subscriber.pending.size > MAX_PENDING || subscriber.pendingBytes > MAX_PENDING_BYTES) {
    const oldest = [...subscriber.pending].find(([, update]) => !failed(update.event))
      ?? subscriber.pending.entries().next().value;
    if (!oldest) break;
    subscriber.pendingBytes -= oldest[1].bytes;
    subscriber.pending.delete(oldest[0]);
    if (failed(oldest[1].event)) report("pending snapshot errors exceeded the view budget");
  }
}

function schedule(subscriber: Subscriber, wait = subscriber.wait) {
  if (subscriber.running || subscriber.timer || !active(subscriber) || !subscribers.has(subscriber)) return;
  subscriber.timer = setTimeout(() => {
    subscriber.timer = undefined;
    if (active(subscriber) && subscribers.has(subscriber)) void read(subscriber);
  }, wait);
}

/// Takes every pending update in one pass so a burst over many keys draws once after the wait.
/// A receiver that takes no event argument rereads its state and runs once; one that reads the
/// event gets each update in arrival order, without waiting between them.
async function read(subscriber: Subscriber) {
  const taken = [...subscriber.pending];
  if (!taken.length) return;
  subscriber.pending.clear();
  subscriber.pendingBytes = 0;
  subscriber.running = true;
  let errored = false;

  const keep = (from: number) => {
    for (const [key, update] of taken.slice(from)) if (!subscriber.pending.has(key)) markDirty(subscriber, update.event);
  };

  if (subscriber.receive.length === 0) {
    try { await subscriber.receive(taken[taken.length - 1][1].event); } catch (error) { report(error); errored = true; keep(0); }
  } else {
    for (const [index, [key, { event }]] of taken.entries()) {
      if (!subscribers.has(subscriber)) break;
      if (!active(subscriber)) { keep(index); break; }

      try { await subscriber.receive(event); } catch (error) {
        report(error);
        errored = true;
        if (!subscriber.pending.has(key)) markDirty(subscriber, event);
      }
    }
  }

  subscriber.running = false;
  subscriber.failures = errored ? subscriber.failures + 1 : 0;
  if (subscriber.pending.size) schedule(subscriber, subscriber.failures ? Math.min(1000 * 2 ** Math.min(subscriber.failures - 1, 5), 30000) : subscriber.wait);
}

function stop() {
  clearTimeout(retry);
  const current = reader;
  reader = null;
  resumeFrom = null;
  clearViews();
  if (current) void import("./tauri.ts").then(({ invoke }) => invoke("mesh_topics", { id: current.id, topics: null })).catch(() => {});
}

if (typeof window !== "undefined") {
  onShownChange(refreshTopicSubscriptions);
  onShownChange(refreshTopicSubscriptions, true);
  window.addEventListener("pagehide", stop);
}
