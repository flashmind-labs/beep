import { createStore } from "zustand";

export type PluginSlotName = "composer";
export type PluginAlign = "start" | "center" | "end";

export type PluginNode =
  | { kind: "image"; src: string; width: number; height: number; pixelated: boolean }
  | { kind: "text"; text: string }
  | { kind: "row"; gap: number; children: PluginNode[] };

/// One node a plugin put in a slot, as the desktop publishes it.
export interface Placement {
  plugin: string;
  slot: PluginSlotName;
  align: PluginAlign;
  offset: number;
  sink: number;
  node: PluginNode;
}

/// Images arrive as data URLs; anything else is refused.
const IMAGE_PREFIX = "data:image/";
/// Rows nest no deeper than this.
const MAX_DEPTH = 8;

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const isLength = (value: unknown): value is number => typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 65535;

function parseNode(raw: unknown, depth: number): PluginNode | null {
  if (!isRecord(raw) || depth > MAX_DEPTH) return null;

  if (raw.kind === "image") {
    const { src, width, height, pixelated } = raw;
    if (typeof src !== "string" || !src.startsWith(IMAGE_PREFIX)) return null;
    if (!isLength(width) || !isLength(height) || typeof pixelated !== "boolean") return null;
    return { kind: "image", src, width, height, pixelated };
  }

  if (raw.kind === "text") {
    return typeof raw.text === "string" ? { kind: "text", text: raw.text } : null;
  }

  if (raw.kind === "row") {
    if (!isLength(raw.gap) || !Array.isArray(raw.children)) return null;
    const children: PluginNode[] = [];
    for (const child of raw.children) {
      const node = parseNode(child, depth + 1);
      if (node === null) return null;
      children.push(node);
    }
    return { kind: "row", gap: raw.gap, children };
  }

  return null;
}

function parsePlacement(raw: unknown): Placement | null {
  if (!isRecord(raw)) return null;
  const { plugin, slot, align, offset, sink } = raw;
  if (typeof plugin !== "string" || slot !== "composer") return null;
  if (align !== "start" && align !== "center" && align !== "end") return null;
  if (!isLength(offset) || !isLength(sink)) return null;

  const node = parseNode(raw.node, 0);
  return node === null ? null : { plugin, slot, align, offset, sink, node };
}

/// Reads the host's payload, dropping each placement that is not valid.
export function parsePlacements(payload: unknown): Placement[] {
  if (!Array.isArray(payload)) return [];
  return payload.flatMap(raw => {
    const placement = parsePlacement(raw);
    return placement === null ? [] : [placement];
  });
}

export function placementsFor(placements: readonly Placement[], slot: PluginSlotName): Placement[] {
  return placements.filter(placement => placement.slot === slot);
}

interface PluginsState {
  placements: Placement[];
}

/// What plugins have placed, shared by every slot.
export const pluginsStore = createStore<PluginsState>(() => ({ placements: [] }));

/// The page's side of the desktop: reading placements and hearing changes.
export interface PluginsBridge {
  invoke(cmd: string): Promise<unknown>;
  listen(event: string, handler: (event: { payload: unknown }) => void): Promise<unknown>;
}

/// Fills the store from the desktop now and on every `plugins-changed`.
export function connectPlugins(bridge: PluginsBridge): Promise<void> {
  const show = (payload: unknown) => pluginsStore.setState({ placements: parsePlacements(payload) });
  const subscribed = bridge.listen("plugins-changed", event => show(event.payload)).then(() => undefined);
  const first = bridge.invoke("plugins_placements").then(show);
  return Promise.all([subscribed, first]).then(() => undefined).catch(() => undefined);
}

/// A topic plugins follow, as the desktop reports it: `key` null means every key, `machine` null any machine.
export interface WantedTopic {
  name: string;
  key: string | null;
  machine: string | null;
}

/// The session whose message box is shown, published as `ui.composer`.
export interface ComposerShown {
  machine: string;
  session: string;
}

/// What the page publishes itself: a `ui.` topic and its value. `null` means the thing went away.
export type UiUpdate = { name: "ui.composer"; data: ComposerShown | null };

/// The mesh topics the page follows for plugins; the desktop refuses others too.
const MESH_NAMES: ReadonlySet<string> = new Set(["agents", "agent"]);
const WORD = /^[A-Za-z0-9._-]{1,128}$/;
const MAX_KEY = 1024;
const LOCAL_PREFIX = "ui.";

function parseWanted1(raw: unknown): WantedTopic | null {
  if (!isRecord(raw)) return null;
  const { name, key, machine } = raw;
  if (typeof name !== "string" || !WORD.test(name)) return null;
  if (key !== null && (typeof key !== "string" || key.length === 0 || key.length > MAX_KEY || key.includes("\0"))) return null;
  if (machine !== null && (typeof machine !== "string" || !WORD.test(machine))) return null;
  return { name, key, machine };
}

/// Reads the desktop's list of wanted topics, dropping each entry that is not valid.
export function parseWanted(payload: unknown): WantedTopic[] {
  if (!Array.isArray(payload)) return [];
  return payload.flatMap(raw => {
    const wanted = parseWanted1(raw);
    return wanted === null ? [] : [wanted];
  });
}

/// The topics to follow on the mesh: `ui.` topics are published by the page, other names are refused.
export function meshTopics(wanted: readonly WantedTopic[]): WantedTopic[] {
  return wanted.filter(topic => !topic.name.startsWith(LOCAL_PREFIX) && MESH_NAMES.has(topic.name));
}

const wantedKey = (topic: WantedTopic) => JSON.stringify([topic.name, topic.key, topic.machine]);

/// The page's side of the topic bridge: the desktop, and the app's mesh subscriptions.
export interface TopicsBridge extends PluginsBridge {
  invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown>;
  /// Follows `topic` on the mesh; `receive` gets the sending machine and the update's data. Returns the stop.
  follow(topic: WantedTopic, receive: (machine: string, data: unknown) => void): () => void;
}

let sendUi: ((update: UiUpdate) => void) | null = null;
const waitingUi = new Map<string, UiUpdate>();

/// Publishes a `ui.` topic to plugins. Before the bridge connects, the latest value of each waits.
export function publishUi(update: UiUpdate): void {
  if (sendUi === null) waitingUi.set(update.name, update);
  else sendUi(update);
}

/// Follows on the mesh what plugins want, forwarding each update to the desktop, and sends `ui.` updates.
export function connectPluginTopics(bridge: TopicsBridge): Promise<void> {
  const stops = new Map<string, () => void>();
  let heard = false;

  const forward = (topic: WantedTopic, machine: string | null, data: unknown) =>
    bridge.invoke("plugins_publish", { update: { topic, machine, data } }).catch(() => undefined);

  const sync = (wanted: readonly WantedTopic[]) => {
    const next = new Map(meshTopics(wanted).map(topic => [wantedKey(topic), topic]));
    for (const [key, stop] of stops) {
      if (next.has(key)) continue;
      stop();
      stops.delete(key);
    }
    for (const [key, topic] of next) {
      if (stops.has(key)) continue;
      stops.set(key, bridge.follow(topic, (machine, data) => void forward(topic, machine, data)));
    }
  };

  sendUi = update => void forward({ name: update.name, key: null, machine: null }, null, update.data);
  for (const update of waitingUi.values()) sendUi(update);
  waitingUi.clear();

  const subscribed = bridge.listen("plugins-wanted", event => { heard = true; sync(parseWanted(event.payload)); }).then(() => undefined);
  // An event that arrived first is newer than the answer to the read.
  const first = bridge.invoke("plugins_wanted").then(payload => { if (!heard) sync(parseWanted(payload)); });
  return Promise.all([subscribed, first]).then(() => undefined).catch(() => undefined);
}
