import { createStore } from "zustand";
import { browserBackend, createAssetCache, parseAssetRef, type AssetBackend, type AssetRef } from "./assets.ts";
import { isPlay, MAX_FRAMES, parseMotions, parseReactions, type SpriteMotion, type SpritePlay, type SpriteReactions } from "./sprite.ts";

/// A named place in the app. A slot can be on the page many times, each with its own context.
export type PluginSlotName = "composer" | "session.row" | "session.header";
type PluginAlign = "start" | "center" | "end";
type PluginTone = "normal" | "dim" | "accent";
/// How a placement comes and goes: a short fade or rise, or nothing.
type PluginTransition = "none" | "fade" | "rise";

/// Names a function the plugin passed in a tree, valid for one render of one instance.
export type CallbackId = number;

export type PluginNode =
  | { kind: "image"; asset: AssetRef; width: number; height: number; pixelated: boolean }
  | { kind: "sprite"; frames: AssetRef[]; rest: number; width: number; height: number; pixelated: boolean; motions: SpriteMotion[]; reactions: SpriteReactions; play: SpritePlay | null }
  | { kind: "text"; text: string; tone: PluginTone }
  | { kind: "row"; gap: number; children: PluginNode[] }
  | { kind: "button"; label: string; children: PluginNode[]; onPress: CallbackId }
  | { kind: "toggle"; label: string; value: boolean; onChange: CallbackId }
  | { kind: "input"; label: string; value: string; placeholder: string; onChange: CallbackId | null; onSubmit: CallbackId | null };

/// Where a placement sits: on the slot's edge, or in the page's flow where the slot is.
export type PluginPosition = { kind: "edge"; align: PluginAlign; offset: number; sink: number } | { kind: "inline" };

/// One node a plugin put in one instance of a slot, as the desktop publishes it.
export interface Placement {
  plugin: string;
  slot: PluginSlotName;
  /// The key the page reported the instance with.
  instance: string;
  position: PluginPosition;
  enter: PluginTransition;
  leave: PluginTransition;
  node: PluginNode;
}

/// Rows nest no deeper than this.
const MAX_DEPTH = 8;
const MAX_KEY_BYTES = 128;
const SLOTS: ReadonlySet<string> = new Set<PluginSlotName>(["composer", "session.row", "session.header"]);

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const isLength = (value: unknown): value is number => typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 65535;
const isCallback = (value: unknown): value is CallbackId => typeof value === "number" && Number.isInteger(value) && value > 0;
const isSlot = (value: unknown): value is PluginSlotName => typeof value === "string" && SLOTS.has(value);
const byteLength = (text: string) => new TextEncoder().encode(text).length;

function parseChildren(raw: unknown, depth: number): PluginNode[] | null {
  if (!Array.isArray(raw)) return null;
  const children: PluginNode[] = [];
  for (const child of raw) {
    const node = parseNode(child, depth + 1);
    if (node === null) return null;
    children.push(node);
  }
  return children;
}

function parseNode(raw: unknown, depth: number): PluginNode | null {
  if (!isRecord(raw) || depth > MAX_DEPTH) return null;

  if (raw.kind === "image") {
    const { width, height, pixelated } = raw;
    const asset = parseAssetRef(raw.asset);
    if (asset === null || !isLength(width) || !isLength(height) || typeof pixelated !== "boolean") return null;
    return { kind: "image", asset, width, height, pixelated };
  }

  if (raw.kind === "sprite") {
    const { rest, width, height, pixelated } = raw;
    if (!Array.isArray(raw.frames) || raw.frames.length === 0 || raw.frames.length > MAX_FRAMES) return null;
    const frames = raw.frames.map(parseAssetRef);
    if (!frames.every(frame => frame !== null)) return null;
    const motions = parseMotions(raw.motions, frames.length);
    const reactions = parseReactions(raw.reactions, frames.length);
    const play: unknown = raw.play;
    if (motions === null || reactions === null || (play !== null && !isPlay(play, reactions)) || typeof rest !== "number" || !Number.isInteger(rest) || rest < 0 || rest >= frames.length) return null;
    if (!isLength(width) || !isLength(height) || typeof pixelated !== "boolean") return null;
    return { kind: "sprite", frames, rest, width, height, pixelated, motions, reactions, play };
  }

  if (raw.kind === "text") {
    const { text, tone } = raw;
    if (typeof text !== "string" || (tone !== "normal" && tone !== "dim" && tone !== "accent")) return null;
    return { kind: "text", text, tone };
  }

  if (raw.kind === "row") {
    const children = parseChildren(raw.children, depth);
    return isLength(raw.gap) && children !== null ? { kind: "row", gap: raw.gap, children } : null;
  }

  if (raw.kind === "button") {
    const children = parseChildren(raw.children, depth);
    return typeof raw.label === "string" && isCallback(raw.onPress) && children !== null ? { kind: "button", label: raw.label, children, onPress: raw.onPress } : null;
  }

  if (raw.kind === "toggle") {
    return typeof raw.label === "string" && typeof raw.value === "boolean" && isCallback(raw.onChange) ? { kind: "toggle", label: raw.label, value: raw.value, onChange: raw.onChange } : null;
  }

  if (raw.kind === "input") {
    const { label, value, placeholder, onChange, onSubmit } = raw;
    if (typeof label !== "string" || typeof value !== "string" || typeof placeholder !== "string") return null;
    if ((onChange !== null && !isCallback(onChange)) || (onSubmit !== null && !isCallback(onSubmit))) return null;
    return { kind: "input", label, value, placeholder, onChange, onSubmit };
  }

  return null;
}

function parsePosition(raw: unknown): PluginPosition | null {
  if (!isRecord(raw)) return null;
  if (raw.kind === "inline") return { kind: "inline" };
  if (raw.kind !== "edge") return null;
  const { align, offset, sink } = raw;
  if (align !== "start" && align !== "center" && align !== "end") return null;
  return isLength(offset) && isLength(sink) ? { kind: "edge", align, offset, sink } : null;
}

const isTransition = (value: unknown): value is PluginTransition => value === "none" || value === "fade" || value === "rise";

function parsePlacement(raw: unknown): Placement | null {
  if (!isRecord(raw)) return null;
  const { plugin, slot, instance, enter, leave } = raw;
  if (typeof plugin !== "string" || !isSlot(slot)) return null;
  if (typeof instance !== "string" || instance.length === 0 || byteLength(instance) > MAX_KEY_BYTES) return null;

  const position = parsePosition(raw.position);
  const node = parseNode(raw.node, 0);
  // Only the composer stands on an edge; every other slot is in the page's flow.
  if (position === null || node === null || !isTransition(enter) || !isTransition(leave) || (position.kind === "edge") !== (slot === "composer")) return null;
  return { plugin, slot, instance, position, enter, leave, node };
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

/// Names one instance of a slot: the slot, and the key the page reported it with.
export const instanceId = (slot: PluginSlotName, key: string) => `${slot}\n${key}`;

/// The placements of each instance, in the order the host gave them. An instance whose
/// placements did not change keeps the list it had, so a view that shows it stays as it is.
export function indexPlacements(placements: readonly Placement[], previous: ReadonlyMap<string, readonly Placement[]>): Map<string, readonly Placement[]> {
  const grouped = new Map<string, Placement[]>();
  for (const placement of placements) {
    const id = instanceId(placement.slot, placement.instance);
    const group = grouped.get(id);
    if (group) group.push(placement);
    else grouped.set(id, [placement]);
  }

  const index = new Map<string, readonly Placement[]>();
  for (const [id, group] of grouped) {
    const before = previous.get(id);
    index.set(id, before !== undefined && JSON.stringify(before) === JSON.stringify(group) ? before : group);
  }
  return index;
}

interface PluginsState {
  placements: Placement[];
  /// The same placements by instance.
  index: ReadonlyMap<string, readonly Placement[]>;
  /// Blob URLs of the images placements use, by hash, for the ones that have arrived.
  assets: ReadonlyMap<string, string>;
}

/// What plugins have placed, shared by every slot.
export const pluginsStore = createStore<PluginsState>(() => ({ placements: [], index: new Map(), assets: new Map() }));

/// Every image the nodes use, the frames of sprites included.
export function assetRefs(placements: readonly Placement[]): AssetRef[] {
  const refs: AssetRef[] = [];
  const visit = (node: PluginNode) => {
    if (node.kind === "image") refs.push(node.asset);
    else if (node.kind === "sprite") refs.push(...node.frames);
    else if (node.kind === "row" || node.kind === "button") node.children.forEach(visit);
  };
  for (const placement of placements) visit(placement.node);
  return refs;
}

/// The page's side of the desktop: reading placements and hearing changes.
export interface PluginsBridge {
  invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown>;
  listen(event: string, handler: (event: { payload: unknown }) => void): Promise<unknown>;
}

/// Fills the store from the desktop now and on every `plugins-changed`, and fetches the images
/// placements use through `plugins_asset`, once each.
export function connectPlugins(bridge: PluginsBridge, backend: AssetBackend = browserBackend(hash => bridge.invoke("plugins_asset", { hash }))): Promise<void> {
  const cache = createAssetCache(backend, () => pluginsStore.setState({ assets: cache.urls() }));
  const show = (payload: unknown) => {
    const placements = parsePlacements(payload);
    cache.use(assetRefs(placements));
    pluginsStore.setState(state => ({ placements, index: indexPlacements(placements, state.index) }));
  };
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

/// Where a session's turn stands, as the `session.turn` topic tells plugins. `turn` counts the turns
/// that ended: a larger number than before is a new end, the same number is a repeat.
type TurnState = "starting" | "working" | "asking" | "idle" | "ended";
type TurnOutcome = "finished" | "cancelled" | "failed" | "refused" | "token-limit" | "request-limit" | "stopped";
export interface SessionTurn { session: string; state: TurnState; outcome: TurnOutcome | null; turn: number }

/// What `session.turn` is built from: the page the open session shows. `last_stop` is the node's
/// `StopReason`, as the desktop sends it.
export interface TurnSource {
  session: { id: string; status: string; question?: object | null; subagents?: number };
  turns: number;
  last_stop?: string | null;
}

const OUTCOMES: ReadonlyMap<string, TurnOutcome> = new Map([
  ["EndTurn", "finished"], ["Cancelled", "cancelled"], ["Failed", "failed"], ["Refusal", "refused"],
  ["MaxTokens", "token-limit"], ["MaxTurnRequests", "request-limit"],
]);

/// A turn runs while the session works, runs subagents or runs with nothing more known; it waits on
/// the user when it asks. A stop reason this page does not know is `stopped`.
export function sessionTurn({ session, turns, last_stop }: TurnSource): SessionTurn {
  const { id, status, question, subagents } = session;
  const asking = status === "asking" || (question ?? null) !== null;
  const running = status === "working" || status === "running" || (status === "idle" && (subagents ?? 0) > 0);
  const state: TurnState = asking ? "asking" : running ? "working" : status === "starting" || status === "ended" ? status : "idle";
  return { session: id, state, outcome: typeof last_stop === "string" ? OUTCOMES.get(last_stop) ?? "stopped" : null, turn: turns };
}

const TURN_TOPIC = "session.turn";
/// The sessions whose latest turn the page keeps for plugins that follow later.
const MAX_TURNS = 256;
const turns = new Map<string, { machine: string; turn: SessionTurn; json: string }>();
let sendTurn: ((machine: string, turn: SessionTurn) => void) | null = null;

/// Publishes `session.turn` for a session the page follows. Only a change is sent, so a turn end that
/// shows again in the next snapshots reaches plugins once.
export function publishTurn(machine: string, source: TurnSource): void {
  const turn = sessionTurn(source);
  const key = JSON.stringify([machine, turn.session]);
  const json = JSON.stringify(turn);
  if (turns.get(key)?.json === json) return;

  turns.delete(key);
  turns.set(key, { machine, turn, json });
  for (const old of turns.keys()) {
    if (turns.size <= MAX_TURNS) break;
    turns.delete(old);
  }
  sendTurn?.(machine, turn);
}

const followsTurn = (topic: WantedTopic, machine: string, { session }: SessionTurn) =>
  (topic.key === null || topic.key === session) && (topic.machine === null || topic.machine === machine);

/// The mesh topics the page follows for plugins; the desktop refuses others too.
const WORD = /^[A-Za-z0-9._-]{1,128}$/;
const MAX_KEY = 1024;

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

const wantedKey = (topic: WantedTopic) => JSON.stringify([topic.name, topic.key, topic.machine]);

/// The page's side of the topic bridge: the desktop.
export interface TopicsBridge extends PluginsBridge {
  invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown>;
}

let sendUi: ((update: UiUpdate) => void) | null = null;
const waitingUi = new Map<string, UiUpdate>();

/// Publishes a `ui.` topic to plugins. Before the bridge connects, the latest value of each waits.
export function publishUi(update: UiUpdate): void {
  if (sendUi === null) waitingUi.set(update.name, update);
  else sendUi(update);
}

/// Sends `ui.` updates and the session turn to the desktop. The desktop follows the mesh topics itself.
export function connectPluginTopics(bridge: TopicsBridge): Promise<void> {
  let heard = false;

  const forward = (topic: WantedTopic, machine: string | null, data: unknown) =>
    bridge.invoke("plugins_publish", { update: { topic, machine, data } }).catch(() => undefined);

  let turnTopics: WantedTopic[] = [];
  const sync = (wanted: readonly WantedTopic[]) => {
    const before = new Set(turnTopics.map(wantedKey));
    turnTopics = wanted.filter(topic => topic.name === TURN_TOPIC);
    // A plugin that follows later gets what the page knows.
    for (const topic of turnTopics.filter(topic => !before.has(wantedKey(topic)))) {
      for (const { machine, turn } of turns.values()) if (followsTurn(topic, machine, turn)) void forward(topic, machine, turn);
    }
  };

  sendUi = update => void forward({ name: update.name, key: null, machine: null }, null, update.data);
  sendTurn = (machine, turn) => {
    for (const topic of turnTopics) if (followsTurn(topic, machine, turn)) void forward(topic, machine, turn);
  };
  for (const update of waitingUi.values()) sendUi(update);
  waitingUi.clear();

  const subscribed = bridge.listen("plugins-wanted", event => { heard = true; sync(parseWanted(event.payload)); }).then(() => undefined);
  // An event that arrived first is newer than the answer to the read.
  const first = bridge.invoke("plugins_wanted").then(payload => { if (!heard) sync(parseWanted(payload)); });
  return Promise.all([subscribed, first]).then(() => undefined).catch(() => undefined);
}

/// The context of an instance of each slot, as the page tells plugins.
interface SessionRef { machine: string; session: string }
interface SessionRowContext extends SessionRef { title: string }

/// One place a slot is on the page now, and what it is about.
export type SlotInstance =
  | { slot: "composer"; key: string; context: SessionRef }
  | { slot: "session.row"; key: string; context: SessionRowContext }
  | { slot: "session.header"; key: string; context: SessionRef };

interface InstanceRef { slot: PluginSlotName; key: string }

/// What changed about the slots on the page. `reset` says the page started over.
export interface InstanceBatch { reset: boolean; shown: SlotInstance[]; gone: InstanceRef[] }

/// The host keeps no more instances of a slot than this.
const MAX_INSTANCES_PER_SLOT = 200;
/// The longest text in a context, and a context as JSON, in bytes.
const MAX_CONTEXT_TEXT = 512;
const MAX_CONTEXT_BYTES = 1024;

/// `text` cut to at most `max` bytes, at a character boundary.
export function clipBytes(text: string, max: number): string {
  if (byteLength(text) <= max) return text;
  let end = text.length;
  while (end > 0 && byteLength(text.slice(0, end)) > max) end--;
  return text.slice(0, end);
}

/// Whether the host takes the instance: a short key, and a small context.
export function instanceFits({ key, context }: SlotInstance): boolean {
  if (key.length === 0 || byteLength(key) > MAX_KEY_BYTES || /[\u0000-\u001f\u007f]/.test(key)) return false;
  return Object.values(context).every(text => byteLength(text) <= MAX_CONTEXT_TEXT) && byteLength(JSON.stringify(context)) <= MAX_CONTEXT_BYTES;
}

/// Which slot instances are on the page, sent to the host in batches. A view calls `show` when
/// its slot appears and `hide` when it goes. Each microtask's changes make one batch, and the
/// first batch after `connect` starts over. An instance shown twice stays until both hide.
export interface InstanceBook {
  /// False when the instance is refused: it does not fit, or its slot holds the most already.
  show(instance: SlotInstance): boolean;
  hide(slot: PluginSlotName, key: string): void;
  connect(send: (batch: InstanceBatch) => void): void;
}

interface Held { instance: SlotInstance; json: string; count: number }

export function createInstanceBook(schedule: (run: () => void) => void): InstanceBook {
  const held = new Map<string, Held>();
  const perSlot = new Map<PluginSlotName, number>();
  const changed = new Map<string, SlotInstance>();
  const gone = new Map<string, InstanceRef>();
  let send: ((batch: InstanceBatch) => void) | null = null;
  let started = false;
  let queued = false;

  const flush = () => {
    queued = false;
    if (send === null) return;
    send({ reset: !started, shown: [...changed.values()], gone: [...gone.values()] });
    started = true;
    changed.clear();
    gone.clear();
  };
  const queue = () => {
    if (queued || send === null) return;
    queued = true;
    schedule(flush);
  };

  return {
    show(instance) {
      if (!instanceFits(instance)) return false;
      const id = instanceId(instance.slot, instance.key);
      const json = JSON.stringify(instance.context);
      const was = held.get(id);

      if (was === undefined) {
        const count = perSlot.get(instance.slot) ?? 0;
        if (count >= MAX_INSTANCES_PER_SLOT) return false;
        perSlot.set(instance.slot, count + 1);
        held.set(id, { instance, json, count: 1 });
      } else {
        was.count++;
        if (was.json === json) return true;
        was.instance = instance;
        was.json = json;
      }

      changed.set(id, instance);
      gone.delete(id);
      queue();
      return true;
    },
    hide(slot, key) {
      const id = instanceId(slot, key);
      const was = held.get(id);
      if (was === undefined || --was.count > 0) return;

      held.delete(id);
      perSlot.set(slot, (perSlot.get(slot) ?? 1) - 1);
      changed.delete(id);
      gone.set(id, { slot, key });
      queue();
    },
    connect(next) {
      send = next;
      // Everything shown so far is news to the host.
      for (const [id, entry] of held) changed.set(id, entry.instance);
      queued = true;
      schedule(flush);
    },
  };
}

/// What the user did to a control a plugin drew.
export type PluginAction = { kind: "press" } | { kind: "toggle"; value: boolean } | { kind: "text"; value: string };
export interface PluginEvent { plugin: string; id: CallbackId; action: PluginAction }

/// A text field's draft. `pending` holds the values sent to the plugin that it has not echoed back yet, oldest first.
export interface InputDraft { text: string; pending: readonly string[] }

/// Values sent and not yet echoed, at most. An echo older than these counts as a value the plugin set.
const MAX_PENDING_ECHOES = 64;

export const startDraft = (value: string): InputDraft => ({ text: value, pending: [] });

/// The user typed `value`: it shows at once, and the plugin is sent it.
export function typeInto(draft: InputDraft, value: string): InputDraft {
  return { text: value, pending: [...draft.pending, value].slice(-MAX_PENDING_ECHOES) };
}

/// The plugin's value is now `value`. If it is one the page sent, it is an echo: it and the older ones are
/// done, and what the user typed since stays. Anything else is the plugin's own value, such as a field it
/// clears after a submit, and replaces the draft. A plugin that changes the text it was sent, to "a" after
/// it got "A", looks like an echo when the page sent "a" before.
export function followPlugin(draft: InputDraft, value: string): InputDraft {
  const echoed = draft.pending.indexOf(value);
  if (echoed < 0) return startDraft(value);

  return { text: draft.text, pending: draft.pending.slice(echoed + 1) };
}

/// The slots on the page, reported to the host once `connectPluginSlots` ran.
export const instanceBook = createInstanceBook(run => queueMicrotask(run));

let sendEvent: ((event: PluginEvent) => void) | null = null;

/// Passes the user's use of a control to the plugin that drew it. Before the bridge connects, it is dropped.
export function sendPluginEvent(event: PluginEvent): void {
  sendEvent?.(event);
}

/// The page's side of the slots: the desktop commands that take instances and events.
export interface SlotsBridge {
  invoke(cmd: string, args: Record<string, unknown>): Promise<unknown>;
}

/// Starts reporting the slots on the page, and passing control events, to the desktop.
export function connectPluginSlots(bridge: SlotsBridge): void {
  sendEvent = event => void bridge.invoke("plugins_event", { event }).catch(() => undefined);
  instanceBook.connect(batch => void bridge.invoke("plugins_instances", { batch }).catch(() => undefined));
}

/// What the page needs to tell the desktop whether it is visible.
export interface VisibilityBridge {
  invoke(cmd: string, args: Record<string, unknown>): Promise<unknown>;
  /// Whether this window is on screen now.
  shown(): boolean;
  /// Calls `fn` each time that changes.
  onChange(fn: (shown: boolean) => void): void;
}

/// Tells the desktop when this window shows or hides. While none shows, plugins stay quiet.
export function connectPluginVisibility(bridge: VisibilityBridge): void {
  const tell = (visible: boolean) => void bridge.invoke("plugins_visible", { visible }).catch(() => undefined);
  tell(bridge.shown());
  bridge.onChange(tell);
}
