// The media pane of a coding agent session: what kind of file a path
// names, where a path the agent wrote points, and where the pane sits
// (docked right or at the bottom, how big, folded or not), kept per window.

/// What the pane shows a file as.
export type MediaKind = "image" | "video" | "model" | "pdf";

const KINDS: Record<string, MediaKind> = {
  png: "image", jpg: "image", jpeg: "image", gif: "image", webp: "image", avif: "image",
  mp4: "video", m4v: "video", webm: "video", mov: "video",
  glb: "model", gltf: "model", obj: "model", stl: "model",
  pdf: "pdf",
};

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

/// The pane's place in one window. A size of null is the default for the
/// space there is.
export interface PaneLayout {
  dock: Dock;
  right: number | null;
  bottom: number | null;
  hidden: boolean;
}

export const DEFAULT_LAYOUT: PaneLayout = { dock: "right", right: null, bottom: null, hidden: false };

/// The narrowest a right pane and the conversation beside it get.
export const RIGHT_MIN = 280;
export const CONV_MIN_WIDTH = 360;
/// The lowest a bottom pane and the conversation above it get.
export const BOTTOM_MIN = 150;
export const CONV_MIN_HEIGHT = 200;
/// Narrower than this, the pane goes under the conversation whatever its dock.
export const SIDE_BY_SIDE_MIN = RIGHT_MIN + CONV_MIN_WIDTH + 60;

/// A stored layout, read back; anything that does not read is the default.
export function readLayout(raw: string | null): PaneLayout {
  let v: unknown;
  try { v = raw ? JSON.parse(raw) : null; } catch { v = null; }
  if (!v || typeof v !== "object") return { ...DEFAULT_LAYOUT };
  const o = v as Record<string, unknown>;
  const size = (x: unknown) => typeof x === "number" && Number.isFinite(x) && x > 0 ? Math.round(x) : null;
  return {
    dock: o.dock === "bottom" ? "bottom" : "right",
    right: size(o.right),
    bottom: size(o.bottom),
    hidden: o.hidden === true,
  };
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
