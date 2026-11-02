// A project's board: one square tile per kind of thing in the project, each
// with its count and the few rows that matter now. A tile's header opens its
// full list in place of the board. center.ts owns the page and decides when
// the board shows; this file holds the tiles' rows, as pure functions the
// tests check, and draws the grid.
import { prLabel } from "./project.ts";
import type { TaskRow } from "./project.ts";
import { machineSync } from "./home.ts";
import type { RunningAgent, SyncRow } from "./home.ts";
import { el } from "./projects-state.ts";
import type { SessionRow, WhereRow } from "./projects-state.ts";

/// A chip on a tile row: its class and text.
export interface TileChip {
  cls: string;
  text: string;
}

/// One row of a tile: an optional dot, its text with an optional code
/// before it, and a chip or a muted note. A row with `url` opens it.
export interface TileRow {
  text?: string;
  code?: string;
  url?: string;
  chip?: TileChip | null;
  note?: string;
  dot?: string;
  title?: string;
  /// What the row is about, such as a machine's id, for `TileOptions.menu`.
  id?: string;
}

/// A tile's count, its rows, and what it says without any.
export interface TileData {
  count: string;
  rows: TileRow[];
  empty: string;
}

/// How a tile draws: how many rows before "more", and what opens a row's link.
export interface TileOptions {
  rows?: number;
  openUrl?: ((url: string) => unknown) | null;
  /// Right-clicking a row with an `id` opens this menu.
  menu?: ((row: TileRow, e: MouseEvent) => void) | null;
}

/// Rows a tile shows before its full list.
export const TILE_ROWS = 4;

/// Open task states, in the order the tile lists them.
const OPEN: Record<string, number> = { running: 0, blocked: 1, review: 2, planned: 3 };

/// The Tasks tile: open tasks, running ones first. A chip marks only what
/// waits on the user.
export function taskTile(tasks: readonly Pick<TaskRow, "title" | "state">[] | null | undefined): TileData {
  const list = tasks || [];
  const open = list.filter(t => t.state in OPEN).sort((a, b) => OPEN[a.state] - OPEN[b.state]);
  const running = list.filter(t => t.state === "running").length;
  const count = running ? `${running} running` : open.length ? `${open.length} open` : list.length ? "all done" : "";
  const rows = open.map(t => ({
    text: t.title,
    chip: t.state === "review" ? { cls: "wait", text: "review" } : t.state === "blocked" ? { cls: "wait", text: "blocked" } : null,
  }));
  return { count, rows, empty: list.length ? "Nothing open" : "No tasks yet" };
}

/// The Pull requests tile: the PRs the project's tasks opened, those still
/// in work first. A task in review waits on the user.
export function prTile(tasks: readonly (Partial<Pick<TaskRow, "pr_url">> & Pick<TaskRow, "title" | "state">)[] | null | undefined): TileData {
  const withPr = (tasks || []).filter((t): t is { pr_url: string; title: string; state: string } => !!t.pr_url && t.state !== "dropped");
  const live = withPr.filter(t => t.state !== "done");
  const rows = [...live, ...withPr.filter(t => t.state === "done")].map(t => ({
    code: prLabel(t.pr_url).replace(/^PR /, ""),
    text: t.title,
    url: t.pr_url,
    chip: t.state === "review" ? { cls: "wait", text: "review" } : null,
    note: t.state === "done" ? "done" : t.state === "review" ? "" : t.state,
  }));
  return { count: live.length ? `${live.length} open` : "", rows, empty: "No pull requests from tasks yet" };
}

/// A machine row as the tile reads it: `whereRows` gives them.
type Where = Pick<WhereRow, "kind" | "name"> & Partial<Pick<WhereRow, "id" | "folder" | "text">>;

/// The Machines tile: every machine with where the project is there.
/// `rows` is `whereRows`; `sync`, the project's `project_sync` rows, marks
/// a machine still fetching it.
export function machineTile(rows: readonly Where[] | null | undefined, sync?: readonly Pick<SyncRow, "project" | "machine_id" | "status" | "files" | "pending">[] | null): TileData {
  const list = rows || [];
  const here = list.filter(r => r.kind === "here").length;
  const note = (r: Where) => {
    const s = machineSync(r.id, sync);
    if (s) return s.pct == null ? "scanning" : `syncing ${s.pct}%`;
    return r.kind === "here" ? folderName(r.folder) : r.kind === "checking" ? "checking" : r.kind === "offline" ? "offline" : "not here";
  };
  return {
    count: list.length ? `${here}/${list.length}` : "",
    rows: list.map(r => ({
      id: r.id,
      text: r.name,
      dot: r.kind === "here" ? "" : "off",
      note: note(r),
      title: r.folder ? `${r.name}: ${r.folder}` : r.text || r.name,
    })),
    empty: "No machine has it yet",
  };
}

/// The Agents tile: the coding agents running in the project's folders
/// now, then its past sessions, newest first.
export function agentTile(
  running: readonly Partial<Pick<RunningAgent, "title" | "cwd" | "agent" | "machine_name" | "status">>[] | null | undefined,
  past: readonly Partial<Pick<SessionRow, "title" | "status">>[] | null | undefined,
): TileData {
  const now = running || [], before = past || [];
  const rows: TileRow[] = [
    ...now.map(r => ({ text: r.title || folderName(r.cwd) || r.agent, note: r.machine_name, chip: r.status === "asking" ? { cls: "wait", text: "asks" } : null })),
    ...before.map(s => ({ text: s.title || "Untitled session", note: s.status === "ended" ? "ended" : s.status || "" })),
  ];
  const count = now.length ? `${now.length} running` : before.length ? `${before.length} past` : "";
  return { count, rows, empty: "No agent has worked here yet" };
}

/// The last folder of a path.
function folderName(path: string | null | undefined) { return String(path || "").split(/[\\/]/).filter(Boolean).pop() || ""; }

// ---------- Drawing ----------

const ICONS: Record<string, string> = {
  tasks: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 11l3 3 8-8"/><path d="M20 12v7a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2h11"/></svg>',
  prs: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5v7M18 15.5V9a3 3 0 00-3-3h-4"/><path d="M13 3l-2 3 2 3"/></svg>',
  issues: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="1.5" fill="currentColor"/></svg>',
  machines: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/></svg>',
  agents: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>',
  branches: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="5" r="2.5"/><circle cx="6" cy="19" r="2.5"/><circle cx="18" cy="7" r="2.5"/><path d="M6 7.5v9M18 9.5c0 5-6 4-11 7.5"/></svg>',
  terminals: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 10l3 2-3 2M13 14h4"/></svg>',
};

function icon(svg: string) {
  const s = el("span");
  s.style.display = "inline-flex";
  s.innerHTML = svg; // fixed markup from this file, no server text
  return s.firstChild;
}

/// One row of a tile: an optional dot, its text, a chip or a muted note.
function row(r: TileRow, { openUrl, menu }: TileOptions) {
  const text = el("span", "t", r.code ? el("code", "", r.code) : null, r.code ? " " : null, r.text);
  text.title = String(r.title || r.text);
  const line = el("div", "pb-r", r.dot != null ? el("span", `ddot${r.dot ? " " + r.dot : ""}`) : null, text,
    r.chip ? el("span", `pb-chip ${r.chip.cls}`, r.chip.text) : r.note ? el("span", "k", r.note) : null);
  if (r.url && openUrl) {
    line.classList.add("link");
    line.addEventListener("click", () => openUrl(r.url!));
  }
  if (r.id && menu) line.addEventListener("contextmenu", e => menu(r, e));
  return line;
}

/// One tile: its header opens the full list with `open(key)`.
export function tile(key: string, title: string, data: TileData, open: (key: string) => void, opts: TileOptions = {}) {
  const shown = opts.rows ?? TILE_ROWS;
  const head = el("button", "pb-h", icon(ICONS[key] || ICONS.tasks), el("span", "", title), el("span", "c", data.count || ""));
  head.type = "button";
  head.title = `All ${title.toLowerCase()}`;
  head.addEventListener("click", () => open(key));
  const rows = data.rows.slice(0, shown).map(r => row(r, opts));
  if (!rows.length) rows.push(el("div", "pb-r none", el("span", "t", data.empty)));
  const more = data.rows.length > shown ? el("div", "pb-r more", el("span", "t", `${data.rows.length - shown} more`)) : null;
  const box = el("section", "pb-tile", head, el("div", "pb-rows", ...rows, more));
  box.dataset.tile = key;
  return box;
}

/// The way back from an opened tile to the board.
export function backRow(back: () => void) {
  const b = el("button", "pb-back", "Board", el("kbd", "", "esc"));
  b.type = "button";
  b.addEventListener("click", back);
  return b;
}
