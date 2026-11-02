// A project's file sync: the board's File sync tile, and its full view with
// each machine's state, transfer rate and time left, the changes still
// waiting, the conflicts with a way to keep either side, and Pause/Resume
// for every machine. center.ts draws the page; this file keeps the sync
// detail and reads it again while the view is open.
import type { invoke as Invoke } from "./tauri.ts";
import { bytes } from "./home.ts";
import type { SyncRow } from "./home.ts";
import { reason } from "./projects-state.ts";
import type { MakeButton, MakeEl } from "./projects-state.ts";
import type { TileData, TileRow } from "./board.ts";

/// How far a machine is through what it brings in line now (ProgressRow).
export interface SyncProgress {
  files_done: number;
  files_total: number;
  bytes_done: number;
  bytes_total: number;
  /// Bytes a second; 0 before it can tell.
  rate: number;
  eta_secs: number | null;
}

/// One machine's sync of the project (MachineDetailRow).
export interface MachineDetail {
  machine_id: string;
  machine_name: string;
  folder: string;
  /// `offline`, `scanning`, `syncing`, `in_sync` or `failed`.
  status: string;
  files: number;
  bytes: number;
  pending: number;
  conflicts: string[];
  error: string | null;
  hint: string | null;
  paused: boolean;
  progress: SyncProgress | null;
  waiting: string[];
  can_resolve: boolean;
}

/// A project's sync, as `project_sync_detail` gives it (SyncDetailView).
export interface SyncDetail {
  project: string;
  pinned: boolean;
  paused: boolean;
  machines: MachineDetail[];
}

/// One conflict copy, the file it stands beside, and the machine that can
/// settle it, or null when none can.
export interface Conflict {
  path: string;
  original: string;
  on: string[];
  resolver: string | null;
}

/// What the window hands `makeFileSync`.
export interface FileSyncDeps {
  invoke: typeof Invoke;
  h: MakeEl;
  button: MakeButton;
  redraw: () => void;
}

/// How often the open view reads the sync again.
const POLL_MS = 2000;
/// How old the board's tile lets the sync get before it reads it again.
const TILE_MAX_AGE_MS = 30_000;

/// "2.1 MB/s", or "" before the rate is known.
export function rateText(rate: number | null | undefined) {
  return rate && rate > 0 ? `${bytes(rate)}/s` : "";
}

/// "under a minute", "about 1 min", "about 12 min", "about 2 h", or ""
/// before it can tell.
export function timeLeft(secs: number | null | undefined) {
  if (secs == null) return "";
  if (secs < 45) return "under a minute";
  if (secs < 90) return "about 1 min";
  if (secs < 3600) return `about ${Math.round(secs / 60)} min`;
  const h = Math.round(secs / 3600);
  return `about ${h} h`;
}

/// How far a machine is, 0 to 100, or null while it is not moving files.
export function syncPct(m: Pick<MachineDetail, "status" | "progress" | "files" | "pending">) {
  if (m.status !== "syncing") return null;
  const p = m.progress;
  if (p && p.bytes_total > 0) return Math.min(100, Math.floor((100 * p.bytes_done) / p.bytes_total));
  if (p && p.files_total > 0) return Math.min(100, Math.floor((100 * p.files_done) / p.files_total));
  const total = m.files + m.pending;
  return total ? Math.floor((100 * m.files) / total) : 0;
}

/// A machine's chip: "paused", "syncing 23%", "scanning", "in sync",
/// "offline" or "failed", and whether it asks for a look.
export function stateChip(m: Pick<MachineDetail, "status" | "paused" | "progress" | "files" | "pending" | "conflicts">): { text: string; wait: boolean } {
  if (m.status === "offline") return { text: "offline", wait: false };
  if (m.paused) return { text: "paused", wait: false };
  if (m.status === "failed") return { text: "failed", wait: true };
  if (m.status === "scanning") return { text: "scanning", wait: false };
  if (m.status === "syncing") return { text: `syncing ${syncPct(m)}%`, wait: false };
  if (m.conflicts.length) return { text: "in sync", wait: true };
  return { text: "in sync", wait: false };
}

/// "1 change waiting", "3 changes waiting".
function changes(n: number) { return `${n} ${n === 1 ? "change" : "changes"} waiting`; }

/// The muted line under a machine: files and bytes, or how far it is with
/// the rate and time left, or what waits.
export function machineMeta(m: Pick<MachineDetail, "status" | "paused" | "progress" | "files" | "bytes" | "pending" | "waiting">) {
  const p = m.progress;
  if (m.status === "syncing" && p && !m.paused) {
    return [
      `${p.files_done.toLocaleString("en-US")} of ${p.files_total.toLocaleString("en-US")} files`,
      p.bytes_total ? `${bytes(p.bytes_done)} of ${bytes(p.bytes_total)}` : "",
      rateText(p.rate),
      timeLeft(p.eta_secs),
    ].filter(Boolean);
  }
  const left = Math.max(m.pending, m.waiting.length);
  // An offline machine's counts are not known.
  if (m.status === "offline") return left ? [changes(left)] : [];
  const out = [`${m.files.toLocaleString("en-US")} ${m.files === 1 ? "file" : "files"}`, bytes(m.bytes)];
  if (left) out.push(changes(left));
  return out;
}

/// The file conflict copy `path` stands beside: `notes.txt` for
/// `notes (conflict 1a2b3c4d).txt`; the path itself when it is not one.
export function originalOf(path: string) {
  const m = /^(.*) \(conflict [0-9a-f]{8}\)([^/]*)$/.exec(path);
  return m && !m[1].endsWith("/") && m[1] !== "" ? m[1] + m[2] : path;
}

/// Every conflict copy once, with the machines that have it and the first
/// online one that can settle it.
export function conflictList(d: Pick<SyncDetail, "machines"> | null | undefined): Conflict[] {
  const out = new Map<string, Conflict>();
  for (const m of d?.machines || []) {
    for (const path of m.conflicts) {
      let c = out.get(path);
      if (!c) out.set(path, c = { path, original: originalOf(path), on: [], resolver: null });
      c.on.push(m.machine_id);
      if (!c.resolver && m.can_resolve && m.status !== "offline") c.resolver = m.machine_id;
    }
  }
  return [...out.values()].sort((a, b) => a.path.localeCompare(b.path));
}

/// The changes still waiting, each path once, with the machines it waits on.
export function waitingList(d: Pick<SyncDetail, "machines"> | null | undefined) {
  const out = new Map<string, string[]>();
  for (const m of d?.machines || []) for (const path of m.waiting) out.set(path, [...(out.get(path) || []), m.machine_name]);
  return [...out].map(([path, machines]) => ({ path, machines }));
}

type RowLike = Pick<SyncRow, "machine_id" | "status" | "files" | "pending">;

/// The board's File sync tile: one row per machine with its chip, from the
/// detail once it is read, else from the `project_sync` rows. `names` gives
/// a machine's name by id for those rows.
export function fileSyncTile(d: SyncDetail | null | undefined, rows?: readonly RowLike[] | null, names?: ReadonlyMap<string, string> | null): TileData {
  const machines: MachineDetail[] = d ? d.machines : (rows || []).map(r => ({
    machine_id: r.machine_id, machine_name: names?.get(r.machine_id) || "Machine", folder: "", status: r.status,
    files: r.files, bytes: 0, pending: r.pending, conflicts: [], error: null, hint: null, paused: false, progress: null, waiting: [], can_resolve: false,
  }));
  if (d === undefined && !machines.length) return { count: "", rows: [], empty: "Loading…" };
  const moving = machines.filter(m => !m.paused && (m.status === "syncing" || m.status === "scanning")).length;
  const conflicts = conflictList({ machines }).length;
  const count = d?.paused ? "paused" : moving ? `${moving} syncing` : conflicts ? `${conflicts} ${conflicts === 1 ? "conflict" : "conflicts"}` : machines.length ? "in sync" : "";
  const out: TileRow[] = machines.map(m => {
    const chip = stateChip(m);
    return {
      id: m.machine_id,
      text: m.machine_name,
      dot: m.status === "offline" ? "off" : "",
      chip: chip.wait ? { cls: "wait", text: chip.text } : null,
      note: chip.wait ? "" : chip.text,
      title: m.folder ? `${m.machine_name}: ${m.folder}` : m.machine_name,
    };
  });
  return { count, rows: out, empty: d?.pinned ? "Pinned: it stays on the machines it is on" : "No machine syncs it" };
}

/// One project's sync: `detail` is undefined while it loads and null from
/// a server too old to give it.
interface ProjectSyncState {
  detail: SyncDetail | null | undefined;
  error: string;
  busy: string | null;
  /// A read is under way.
  loading: boolean;
  /// When the detail was last read, in milliseconds.
  read_at: number;
}

export function makeFileSync(deps: FileSyncDeps) {
  const { invoke, h, button, redraw } = deps;
  const all = new Map<string, ProjectSyncState>();
  let timer: ReturnType<typeof setInterval> | undefined;
  let polling: string | null = null;

  const of = (name: string) => {
    let s = all.get(name);
    if (!s) all.set(name, s = { detail: undefined, error: "", busy: null, loading: false, read_at: 0 });
    return s;
  };

  async function load(name: string) {
    const s = of(name);
    if (s.loading) return;
    s.loading = true;
    try {
      s.detail = await invoke<SyncDetail | null>("project_sync_detail", { project: name });
      s.error = "";
    } catch (e) {
      s.detail ??= null;
      s.error = reason(e);
    } finally {
      s.loading = false;
      s.read_at = Date.now();
    }
    redraw();
  }

  /// The detail for the board's tile, read again once it is old.
  function forTile(name: string) {
    const s = of(name);
    if (Date.now() - s.read_at > TILE_MAX_AGE_MS) load(name);
    return s.detail;
  }

  /// Reads `name` again every few seconds while its view shows; null stops.
  function watch(name: string | null) {
    if (polling === name) return;
    clearInterval(timer);
    polling = name;
    if (!name) return;
    timer = setInterval(() => {
      if (document.visibilityState === "visible") load(name);
    }, POLL_MS);
  }

  async function act(name: string, what: string, run: () => Promise<unknown>) {
    const s = of(name);
    s.busy = what;
    s.error = "";
    redraw();
    try {
      await run();
    } catch (e) {
      s.error = reason(e);
    } finally {
      s.busy = null;
    }
    await load(name);
  }

  function pause(name: string, paused: boolean) {
    return act(name, "pause", async () => {
      of(name).detail = await invoke<SyncDetail>("project_sync_pause", { project: name, paused });
    });
  }

  function resolve(name: string, c: Conflict, keepCopy: boolean) {
    return act(name, c.path, () => invoke("project_sync_resolve", { project: name, machineId: c.resolver, path: c.path, keepCopy }));
  }

  function machineRow(m: MachineDetail) {
    const chip = stateChip(m);
    const pct = m.paused ? null : syncPct(m);
    const head = h("div", "fs-h", h("span", `ddot${m.status === "offline" ? " off" : ""}`), h("b", "", m.machine_name),
      h("span", `pb-chip${chip.wait ? " wait" : ""}`, chip.text));
    const bar = pct == null ? null : h("div", "fs-bar", h("i", ""));
    if (bar) (bar.firstChild as HTMLElement).style.width = `${pct}%`;
    const meta = h("div", "fs-m", ...machineMeta(m).map(x => h("span", "", x)), m.folder ? h("code", "", m.folder) : null);
    const row = h("div", "fs-row", head, bar, meta);
    if (m.error) row.append(h("div", "cc-err", m.error));
    if (m.hint) row.append(h("div", "fs-m", `Rescanned instead of watched. To watch it, run ${m.hint}`));
    return row;
  }

  function conflictRow(name: string, c: Conflict, machines: readonly MachineDetail[]) {
    const s = of(name);
    const where = c.on.map(id => machines.find(m => m.machine_id === id)?.machine_name || "").filter(Boolean).join(", ");
    const text = h("span", "t", h("code", "", c.path), h("span", "", `Another copy of ${c.original}${where ? ` · on ${where}` : ""}`));
    const busy = s.busy === c.path;
    let acts: HTMLElement;
    if (c.resolver) {
      const copy = button(busy ? "Keeping…" : "Keep this copy", "small", () => resolve(name, c, true));
      const orig = button("Keep the original", "small", () => resolve(name, c, false));
      copy.title = `Puts this copy in place of ${c.original} on every machine`;
      orig.title = `Keeps ${c.original} and deletes this copy on every machine`;
      copy.disabled = orig.disabled = !!s.busy || !!s.detail?.paused;
      acts = h("span", "acts", copy, orig);
    } else {
      acts = h("span", "dmuted", "Update beep-node on a machine that has it to settle it here");
    }
    return h("div", "fs-conflict", text, acts);
  }

  /// The full view: Pause or Resume, the machines, the conflicts and what
  /// still waits.
  function panel(name: string): HTMLElement[] {
    const s = of(name);
    if (s.detail === undefined) load(name);
    watch(name);
    const d = s.detail;
    const out: HTMLElement[] = [];
    if (s.error) out.push(h("div", "cc-err", s.error));
    if (d === null) {
      out.push(h("div", "cc-panel cc-empty", h("b", "", "The server needs an update before it shows file sync in detail.")));
      return out;
    }
    if (d === undefined) {
      out.push(h("div", "cc-panel cc-empty", h("span", "", "Loading…")));
      return out;
    }
    const toggle = button(s.busy === "pause" ? (d.paused ? "Resuming…" : "Pausing…") : d.paused ? "Resume" : "Pause", d.paused ? "pri" : "", () => pause(name, !d.paused));
    toggle.disabled = !!s.busy || d.pinned || !d.machines.length;
    toggle.title = d.paused ? "Sync again on every machine" : "Hold sync on every machine; files stay as they are";
    const moving = d.machines.filter(m => !m.paused && m.status === "syncing").length;
    const summary = d.pinned ? "pinned" : d.paused ? "paused on every machine" : moving ? `${moving} syncing` : d.machines.length ? "in sync" : "";
    out.push(h("div", "cc-sec", h("h4", "", "Machines"), h("span", "fs-tools", h("small", "", summary), toggle)));
    out.push(h("div", "cc-panel fs-list", ...(d.machines.length
      ? d.machines.map(machineRow)
      : [h("div", "cc-empty", h("span", "", d.pinned ? "Pinned: the project stays on the machines it is on and never syncs." : "No machine syncs this project. Turn it on from a machine's row in Machines."))])));

    const conflicts = conflictList(d);
    if (conflicts.length) {
      out.push(h("div", "cc-sec", h("h4", "", "Conflicts"), h("small", "", `${conflicts.length} ${conflicts.length === 1 ? "file" : "files"} edited on two machines at once`)));
      out.push(h("div", "cc-panel fs-list", ...conflicts.map(c => conflictRow(name, c, d.machines))));
    }

    const waiting = waitingList(d);
    if (waiting.length) {
      out.push(h("div", "cc-sec", h("h4", "", "Waiting"), h("small", "", "changes a machine still has to bring")));
      out.push(h("div", "cc-panel fs-list", ...waiting.map(w => h("div", "fs-wait", h("code", "", w.path), h("span", "dmuted", w.machines.join(", "))))));
    }
    out.push(h("div", "dmuted fs-note", d.paused
      ? "Paused: files stay as they are, and every machine catches up on resume."
      : "Builds, ignored files and links stay on each machine. A conflict keeps both edits until you pick one."));
    return out;
  }

  return {
    load,
    detail: (name: string) => of(name).detail,
    forTile,
    panel,
    stop: () => watch(null),
  };
}
