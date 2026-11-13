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
