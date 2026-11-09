// Views and plugins share topic subscriptions over the window's duplex node connections.
import { onShownChange, pageShown } from "./active.ts";
import { matchesTopic, normalizeTopic, type Topic } from "./topic-state.ts";
import { clearViews, installViews } from "./snapshot-state.ts";
export { matchesTopic, topic, type Topic } from "./topic-state.ts";

export interface MeshEvent { machine_id: string; topic: Topic; revision: number; query?: number | null; initial?: boolean; data?: unknown }
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
  timer?: ReturnType<typeof setTimeout>;
}

const subscribers = new Set<Subscriber>();
interface Reader {
  id: string;
  wanted: Interest[];
  sent: string | null;
  updating: boolean;
}
let reader: Reader | null = null;
let retry: ReturnType<typeof setTimeout> | undefined;
let queued = false;

const active = (subscriber: Subscriber) => pageShown() && subscriber.active();

/// Omitting a machine subscribes across the mesh. `*` includes plugin namespaces too.
export function subscribeTopics(topics: Topic | Topic[], receive: Receive, machine: string | null = null, when: () => boolean = () => true): () => void {
  return subscribe(topics, receive, machine, when, false);
}

/// Initial query answers return through the requesting read; coalesce subsequent snapshots.
export function subscribeChanges(topics: Topic | Topic[], receive: Receive, machine: string | null = null, when: () => boolean = () => true): () => void {
  return subscribe(topics, receive, machine, when, true);
}

function subscribe(topics: Topic | Topic[], receive: Receive, machine: string | null, when: () => boolean, coalesce: boolean): () => void {
  if (typeof window === "undefined") return () => {};
  const subscriber: Subscriber = {
    interests: (Array.isArray(topics) ? topics : [topics]).map(topic => ({ machine, topic: normalizeTopic(topic) })),
    receive, active: when, coalesce, pending: new Map(), pendingBytes: 0, running: false, failures: 0,
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

function interests() {
  const wanted = [...subscribers].filter(active).flatMap(subscriber => subscriber.interests);
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
  const current: Reader = { id: crypto.randomUUID(), wanted: topics, sent: JSON.stringify(topics), updating: false };
  reader = current;
  const onEvent = new Channel<MeshEvent>();
  let first = true;
  onEvent.onmessage = event => {
    if (reader !== current || !pageShown()) {
      void invoke("mesh_topics", { id: current.id, topics: null }).catch(() => {});
      return;
    }

    if (first) { first = false; current.sent = null; refreshTopicSubscriptions(); }
    installViews(event.data);

    for (const subscriber of subscribers) {
      if (!active(subscriber) || !subscriber.interests.some(interest => (interest.machine === null || interest.machine === event.machine_id)
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
  void invoke("mesh_watch", { id: current.id, topics, onEvent }).catch(e => console.warn("mesh stream:", e)).finally(() => {
    if (reader !== current) return;
    reader = null;
    clearViews();
    clearTimeout(retry);
    if (interests().length) retry = setTimeout(refreshTopicSubscriptions, 5000);
  });
}

// Serialize changes so a late IPC response cannot restore an older set of interests.
async function sendTopics(current: Reader) {
  if (current.updating) return;
  current.updating = true;

  try {
    const { invoke } = await import("./tauri.ts");

    while (reader === current && pageShown()) {
      const wanted = current.wanted, key = JSON.stringify(wanted);
      if (current.sent === key) break;
      await invoke("mesh_topics", { id: current.id, topics: wanted });
      current.sent = key;
    }
  } catch (error) {
    console.warn("mesh topics:", error);

    if (reader === current) {
      clearTimeout(retry);
      retry = setTimeout(refreshTopicSubscriptions, 5000);
    }
  } finally {
    current.updating = false;
  }
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

function schedule(subscriber: Subscriber, wait = 100) {
  if (subscriber.running || subscriber.timer || !active(subscriber) || !subscribers.has(subscriber)) return;
  subscriber.timer = setTimeout(() => {
    subscriber.timer = undefined;
    if (active(subscriber) && subscribers.has(subscriber)) void read(subscriber);
  }, wait);
}

async function read(subscriber: Subscriber) {
  const next = subscriber.pending.entries().next().value;
  if (!next) return;
  const [key, pending] = next;
  const { event } = pending;
  subscriber.pending.delete(key);
  subscriber.pendingBytes -= pending.bytes;
  subscriber.running = true;

  try {
    await subscriber.receive(event);
    subscriber.failures = 0;
  } catch (error) {
    report(error);
    subscriber.failures++;
    if (!subscriber.pending.has(key)) markDirty(subscriber, event);
  } finally {
    subscriber.running = false;
  }

  if (subscriber.pending.size) schedule(subscriber, subscriber.failures ? Math.min(1000 * 2 ** Math.min(subscriber.failures - 1, 5), 30000) : 100);
}

function stop() {
  clearTimeout(retry);
  const current = reader;
  reader = null;
  clearViews();
  if (current) void import("./tauri.ts").then(({ invoke }) => invoke("mesh_topics", { id: current.id, topics: null })).catch(() => {});
}

if (typeof window !== "undefined") {
  onShownChange(refreshTopicSubscriptions);
  onShownChange(refreshTopicSubscriptions, true);
  window.addEventListener("pagehide", stop);
}

/// Commands are sent once. A lost acknowledgement is returned to the caller for reconciliation.
export function meshCommand<T>(machine: string, operation: unknown): Promise<T> {
  return import("./tauri.ts").then(({ invoke }) => invoke<T>("mesh_command", { machine, operation }));
}
