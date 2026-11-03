// The media pane of a coding agent session: what kind of file a path
// names, where a path the agent wrote points, and where the pane sits
// (each tab docked right or at the bottom, how big each dock is, folded
// or not), kept per window.

/// What the pane shows a file as.
export type MediaKind = "image" | "video" | "model" | "pdf";

const KINDS: Record<string, MediaKind> = {
  png: "image", jpg: "image", jpeg: "image", gif: "image", webp: "image", avif: "image",
  mp4: "video", m4v: "video", webm: "video", mov: "video",
  glb: "model", gltf: "model", obj: "model", stl: "model",
  pdf: "pdf",
};

/// A 3D model's animation clip, as its picker names it: by its own
/// name, or its place.
export const clipName = (name: string, i: number) => name.trim() || `Clip ${i + 1}`;

/// `m:ss.t` for a time in seconds, as a clip's scrub bar shows it.
export function clipTime(seconds: number) {
  const t = Math.max(0, Math.floor(seconds * 10 + 1e-6));
  return `${Math.floor(t / 600)}:${String(Math.floor(t / 10) % 60).padStart(2, "0")}.${t % 10}`;
}

/// The extensions the pane opens, for the pattern that finds paths.
export const MEDIA_EXTENSIONS = Object.keys(KINDS);

/// A file's extension, lower case, or "".
export function extension(path: string) {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

/// What the pane shows `path` as, or null for a file it does not open.
export function mediaKind(path: string): MediaKind | null {
  return KINDS[extension(path)] ?? null;
}

/// The last part of a path.
export function baseName(path: string) {
  return path.replace(/\/+$/, "").split("/").pop() || path;
}

/// `path` as the node reads it: absolute, or `~/` for its home. A relative
/// path is the session folder's `cwd`; `.` and `..` are taken out.
export function resolvePath(cwd: string, path: string) {
  if (path === "~" || path.startsWith("~/")) return normalize(path);
  const full = path.startsWith("/") ? path : `${cwd.replace(/\/+$/, "")}/${path}`;
  return normalize(full);
}

function normalize(path: string) {
  const home = path.startsWith("~");
  const parts: string[] = [];
  for (const part of path.split("/").slice(home ? 1 : 0)) {
    if (!part || part === ".") continue;
    if (part === "..") parts.pop();
    else parts.push(part);
  }
  return (home ? "~/" : "/") + parts.join("/");
}

// ----- Where the pane sits -----

export type Dock = "right" | "bottom";

export const DOCKS: readonly Dock[] = ["right", "bottom"];

/// The pane's place in one window.
export interface PaneLayout {
  dock: Dock;
  right: number | null;
  bottom: number | null;
  /// The docks folded to a strip.
  hidden: Record<Dock, boolean>;
  /// The dock each tab was dragged to, by the tab's id, oldest first.
  tabs: Record<string, Dock>;
}

export const DEFAULT_LAYOUT: PaneLayout = { dock: "right", right: null, bottom: null, hidden: { right: false, bottom: false }, tabs: {} };

/// The most tabs whose dock is kept: the newest moves.
export const KEPT_TABS = 40;

/// The narrowest a right pane and the conversation beside it get.
export const RIGHT_MIN = 280;
export const CONV_MIN_WIDTH = 360;
/// The lowest a bottom pane and the conversation above it get.
export const BOTTOM_MIN = 150;
export const CONV_MIN_HEIGHT = 200;
/// Narrower than this, the pane goes under the conversation whatever its dock.
export const SIDE_BY_SIDE_MIN = RIGHT_MIN + CONV_MIN_WIDTH + 60;

const isDock = (x: unknown): x is Dock => x === "right" || x === "bottom";

/// Whether a tab's dock is worth keeping: not a picture's, whose id is
/// its data URL and which is not there next time.
const keepable = (id: string) => !id.startsWith("data:") && id.length <= 1024;

/// A stored layout, read back; anything that does not read is the default.
/// One from before tabs had docks folds both docks or neither.
export function readLayout(raw: string | null): PaneLayout {
  let v: unknown;
  try { v = raw ? JSON.parse(raw) : null; } catch { v = null; }
  if (!v || typeof v !== "object" || Array.isArray(v)) return copyLayout(DEFAULT_LAYOUT);
  const o = v as Record<string, unknown>;
  const size = (x: unknown) => typeof x === "number" && Number.isFinite(x) && x > 0 ? Math.round(x) : null;
  const folded = o.hidden && typeof o.hidden === "object" ? o.hidden as Record<string, unknown> : null;
  const tabs: Record<string, Dock> = {};
  if (o.tabs && typeof o.tabs === "object" && !Array.isArray(o.tabs)) {
    for (const [id, d] of Object.entries(o.tabs).slice(-KEPT_TABS)) if (isDock(d) && keepable(id)) tabs[id] = d;
  }
  return {
    dock: o.dock === "bottom" ? "bottom" : "right",
    right: size(o.right),
    bottom: size(o.bottom),
    hidden: folded ? { right: folded.right === true, bottom: folded.bottom === true } : { right: o.hidden === true, bottom: o.hidden === true },
    tabs,
  };
}

function copyLayout(l: PaneLayout): PaneLayout {
  return { ...l, hidden: { ...l.hidden }, tabs: { ...l.tabs } };
}

/// The dock tab `id` sits in, before the window's width has a say.
export function tabDock(l: PaneLayout, id: string): Dock {
  return l.tabs[id] ?? l.dock;
}

/// `l` with tab `id` moved to dock `to`, which unfolds. Only the newest
/// moves are kept, and none of a tab that is not there next time.
export function moveTab(l: PaneLayout, id: string, to: Dock): PaneLayout {
  const out = copyLayout(l);
  delete out.tabs[id];
  if (keepable(id)) out.tabs[id] = to;
  out.hidden[to] = false;
  const ids = Object.keys(out.tabs);
  out.tabs = Object.fromEntries(ids.slice(-KEPT_TABS).map(k => [k, out.tabs[k]]));
  return out;
}

/// The tabs `ids` split between the docks in a shell `width` wide: where each
/// sits, or all at the bottom when a right dock does not fit.
export function splitTabs(l: PaneLayout, ids: readonly string[], width: number, moved: ReadonlyMap<string, Dock> = new Map()): Record<Dock, string[]> {
  const out: Record<Dock, string[]> = { right: [], bottom: [] };
  for (const id of ids) out[effectiveDock(moved.get(id) ?? tabDock(l, id), width)].push(id);
  return out;
}

/// A box on screen.
export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/// The band along an edge that takes a dragged tab: a fifth of the
/// session, and at least this.
export const DROP_BAND = 64;

/// The dock a tab dragged to `x`, `y` goes to in the session `shell`: the
/// right edge docks it right (when a right dock fits), the bottom edge at
/// the bottom, and in the corner the nearer edge. Null anywhere else.
export function dropDock(shell: Box, x: number, y: number, sideBySide: boolean): Dock | null {
  const right = shell.left + shell.width, bottom = shell.top + shell.height;
  if (x < shell.left || x > right || y < shell.top || y > bottom) return null;
  const toRight = right - x, toBottom = bottom - y;
  const inRight = sideBySide && toRight <= Math.max(DROP_BAND, shell.width / 5);
  const inBottom = toBottom <= Math.max(DROP_BAND, shell.height / 5);
  if (inRight && inBottom) return toRight / shell.width < toBottom / shell.height ? "right" : "bottom";
  return inRight ? "right" : inBottom ? "bottom" : null;
}

/// The dock the pane takes in a shell `width` wide: right only when both
/// it and the conversation fit side by side.
export function effectiveDock(dock: Dock, width: number): Dock {
  return dock === "right" && width >= SIDE_BY_SIDE_MIN ? "right" : "bottom";
}

/// The size the pane takes along `total` (the shell's width for a right
/// pane, its height for a bottom one): the one kept, or the default,
/// held between the pane's least and what leaves the conversation room.
export function paneSize(dock: Dock, kept: number | null, total: number) {
  const min = dock === "right" ? RIGHT_MIN : BOTTOM_MIN;
  const room = total - (dock === "right" ? CONV_MIN_WIDTH : CONV_MIN_HEIGHT);
  const wanted = kept ?? Math.round(total * (dock === "right" ? 0.42 : 0.4));
  return Math.round(Math.max(min, Math.min(wanted, Math.max(min, room))));
}

// ----- Zoom and pan of a picture -----

/// How a picture shows: at `scale` of its own size, moved by `x`, `y`
/// from the top left of its box. `fit` keeps it fitted as the box resizes.
export interface ImageView {
  scale: number;
  x: number;
  y: number;
  fit: boolean;
}

export const ZOOM_MIN = 0.05;
export const ZOOM_MAX = 16;

/// The view that fits a `w` by `h` picture in a `bw` by `bh` box, centred,
/// never larger than its own size.
export function fitView(w: number, h: number, bw: number, bh: number): ImageView {
  const scale = w > 0 && h > 0 ? Math.min(1, bw / w, bh / h) : 1;
  return { scale, x: (bw - w * scale) / 2, y: (bh - h * scale) / 2, fit: true };
}

/// The picture at its own size, centred when it is smaller than the box,
/// else from its top left.
export function actualView(w: number, h: number, bw: number, bh: number): ImageView {
  return { scale: 1, x: w < bw ? (bw - w) / 2 : 0, y: h < bh ? (bh - h) / 2 : 0, fit: false };
}

/// `v` zoomed by `factor` about the point `px`, `py` of the box, so the
/// spot under the pointer stays there.
export function zoomAt(v: ImageView, factor: number, px: number, py: number): ImageView {
  const scale = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, v.scale * factor));
  const k = scale / v.scale;
  return { scale, x: px - (px - v.x) * k, y: py - (py - v.y) * k, fit: false };
}
