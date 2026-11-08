// Views and plugins share topic subscriptions over the window's duplex node connections.
import { onShownChange, pageShown } from "./active.ts";

export interface Topic { name: string; key: string | null }
export interface EventId { epoch: string; sequence: number }
export interface MeshEvent { id: EventId; machine_id: string; topic: Topic; resync: boolean; data?: unknown }
interface Interest { machine: string | null; topic: Topic }
type Receive = (event: MeshEvent) => unknown;
interface Subscriber {
  interests: Interest[];
  receive: Receive;
  active: () => boolean;
  coalesce: boolean;
  pending?: MeshEvent;
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

export const topic = (name: string, key: string | null = null): Topic => ({ name, key });
export const matchesTopic = (wanted: Topic, heard: Topic) =>
  (wanted.name === "*" || wanted.name === heard.name) && (wanted.key === null || heard.key === null || wanted.key === heard.key);
const active = (subscriber: Subscriber) => pageShown() && subscriber.active();

/// Omitting a machine subscribes across the mesh. `*` includes plugin namespaces too.
export function subscribeTopics(topics: Topic | Topic[], receive: Receive, machine: string | null = null, when: () => boolean = () => true): () => void {
  return subscribe(topics, receive, machine, when, false);
}

/// Views retain one dirty update while reading, and coalesce bursts before the next read.
export function subscribeChanges(topics: Topic | Topic[], receive: Receive, machine: string | null = null, when: () => boolean = () => true): () => void {
  return subscribe(topics, receive, machine, when, true);
}

function subscribe(topics: Topic | Topic[], receive: Receive, machine: string | null, when: () => boolean, coalesce: boolean): () => void {
  if (typeof window === "undefined") return () => {};
  const subscriber: Subscriber = {
    interests: (Array.isArray(topics) ? topics : [topics]).map(topic => ({ machine, topic })),
    receive, active: when, coalesce, running: false, failures: 0,
  };
  subscribers.add(subscriber);
  refreshTopicSubscriptions();
  return () => {
    clearTimeout(subscriber.timer);
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
    } else if (subscriber.pending) {
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
    for (const subscriber of subscribers) {
      if (!active(subscriber) || !subscriber.interests.some(interest => (interest.machine === null || interest.machine === event.machine_id)
        && (event.resync || matchesTopic(interest.topic, event.topic)))) continue;
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

function markDirty(subscriber: Subscriber, event: MeshEvent) {
  subscriber.pending = subscriber.pending?.resync && !event.resync ? { ...event, topic: topic("*"), resync: true } : event;
}

function schedule(subscriber: Subscriber, wait = 100) {
  if (subscriber.running || subscriber.timer || !active(subscriber) || !subscribers.has(subscriber)) return;
  subscriber.timer = setTimeout(() => {
    subscriber.timer = undefined;
    if (active(subscriber) && subscribers.has(subscriber)) void read(subscriber);
  }, wait);
}

async function read(subscriber: Subscriber) {
  const event = subscriber.pending;
  if (!event) return;
  subscriber.pending = undefined;
  subscriber.running = true;
  try {
    await subscriber.receive(event);
    subscriber.failures = 0;
  } catch (error) {
    report(error);
    subscriber.failures++;
    markDirty(subscriber, subscriber.pending ?? event);
  } finally {
    subscriber.running = false;
  }
  if (subscriber.pending) schedule(subscriber, subscriber.failures ? Math.min(1000 * 2 ** Math.min(subscriber.failures - 1, 5), 30000) : 100);
}

function stop() {
  clearTimeout(retry);
  const current = reader;
  reader = null;
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
