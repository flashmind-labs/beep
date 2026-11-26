// A project's file sync: each machine's state, transfer rate and time left,
// the changes still waiting, the conflicts with a way to keep either side,
// Pause/Resume for every machine, and the board's File sync tile.
import { gitSyncWarning } from "./gitsync-state.ts";
import { syncedBySyncthing } from "./projects-state.ts";
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { AgentButtonItem } from "../review/feedback-view.tsx";
import type { TileData, TileRow } from "./board.ts";
import { PageCache, type SectionStore } from "../ui/pagecache.ts";
import { bytes, plural, reason } from "../ui/format.ts";
import type { DiffViewHandle } from "../review/diff.tsx";
import { compareFiles } from "./compare.ts";
import { loadClocks } from "../machines/clock.ts";
import { onPhone } from "../ui/platform.ts";

/// How far a machine is through what it brings in line now (ProgressRow).
interface SyncProgress {
  files_done: number;
  files_total: number;
  bytes_done: number;
  bytes_total: number;
  /// Bytes a second; 0 before it can tell.
  rate: number;
  eta_secs: number | null;
  /// The file it waits on while it comes.
  file: FileProgress | null;
}

/// How much of one file has come.
interface FileProgress {
  path: string;
  done: number;
  size: number;
}

/// One machine's sync of the project (MachineDetailRow).
export interface MachineDetail {
  machine_id: string;
  machine_name: string;
  folder: string;
  /// `offline`, `scanning`, `syncing`, `in_sync`, `failed`, or as failed with
  /// why: `missing` (its node makes the folder again and fills it),
  /// `parent_missing` (the folder above it is gone too, as with a disk not
  /// mounted) or `syncthing` (Syncthing keeps the folder in step).
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
  /// The folder at its path was replaced and its node can start over with it.
  can_start_over: boolean;
  git_only?: boolean;
}

/// A project's sync, as `project_sync_detail` gives it (SyncDetailView).
export interface SyncDetail {
  ignored_folders?: string[];
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
  /// The first online machine that has it, where an agent can work on it.
  holder: string | null;
}

/// What the window hands `makeFileSync`.
interface FileSyncDeps {
  invoke: typeof Invoke;
  redraw: () => void;
  /// Fills the bar of `project` to have an agent settle a conflict.
  fix?: (project: string, c: Conflict) => void;
  /// The agent buttons that settle a conflict, and a blocked Git sync, at once.
  fixButton?: (project: string, c: Conflict) => AgentButtonItem;
  repairButton?: (project: string, machine: string, error: string) => AgentButtonItem;
  /// Opens the sheet that points `project` on machine `m` at another folder.
  /// Where each project's sync is kept between windows; in memory only
  /// without it.
  kept?: SectionStore<SyncDetail>;
}

/// One project's sync: `detail` is undefined while it loads.
interface ProjectSyncState {
  detail: SyncDetail | undefined;
  error: string;
  busy: string | null;
}

type CompareEntry =
  | { kind: "reading"; token: object }
  | { kind: "note"; text: string }
  | { kind: "diff"; view: DiffViewHandle };

export function makeFileSync(deps: FileSyncDeps) {
  const { invoke, redraw } = deps;
  const all = new Map<string, ProjectSyncState>();
  const loads = new Map<string, Promise<void>>();
  /// The projects whose detail this launch has read or been pushed.
  const current = new Set<string>();
  /// How many details each project was pushed.
  const pushed = new Map<string, number>();
  const compared = new Map<string, CompareEntry>();

  const kept = deps.kept ?? new PageCache(null).section<SyncDetail>("filesync");

  /// A project's sync, from the cache until it is read; the kept one is as
  /// old as the cache says.
  const of = (name: string) => {
    let s = all.get(name);
    if (!s) all.set(name, s = { detail: kept.get(name), error: "", busy: null });
    return s;
  };

  function load(name: string): Promise<void> {
    const pending = loads.get(name);
    if (pending) return pending;
    const read = readList(name);
    loads.set(name, read);
    return read;
  }

  async function readList(name: string) {
    const s = of(name), pushes = pushed.get(name);
    const was = JSON.stringify([s.detail, s.error]);
    const wasDetail = JSON.stringify(s.detail);

    try {
      const detail = await invoke<SyncDetail>("project_sync_detail", { project: name });
      await loadClocks(invoke);
      s.error = "";

      // A detail pushed while this was read is newer than its answer.
      if (pushed.get(name) === pushes) {
        // An unchanged detail keeps its object, so what shows it can skip a draw.
        if (JSON.stringify(detail) !== wasDetail) s.detail = detail;
        kept.put(name, detail);
      }

      current.add(name);
    } catch (e) {
      s.error = reason(e);
    } finally {
      loads.delete(name);
    }

    // A read that found nothing new draws nothing.
    if (was !== JSON.stringify([s.detail, s.error])) redraw();
  }

  /// Reads `name` once in this launch, as a phone shows no project sync, so it reads none: each
  /// read waits on every machine of the project. The `sync` updates carry it after that.
  function ensure(name: string): Promise<void> | undefined {
    if (!onPhone() && !current.has(name) && !loads.has(name)) return load(name);
  }

  function forTile(name: string) {
    ensure(name);
    return of(name).detail;
  }

  /// Takes the detail a `sync` update pushes for `name`; what shows it draws when it differs.
  function take(name: string, detail: SyncDetail) {
    const s = of(name);
    current.add(name);
    pushed.set(name, (pushed.get(name) ?? 0) + 1);
    if (JSON.stringify(detail) === JSON.stringify(s.detail) && !s.error) return;

    s.detail = detail;
    s.error = "";
    kept.put(name, detail);
    redraw();
  }

  async function act(name: string, what: string, run: () => Promise<unknown>) {
    const s = of(name);
    s.busy = what;
    s.error = "";
    redraw();
    let failure = "";

    try {
      await run();
    } catch (e) {
      failure = reason(e);
    } finally {
      s.busy = null;
    }

    // The new detail comes with the `sync` push; the refusal shows meanwhile.
    s.error = failure;
    redraw();
  }

  function pause(name: string, paused: boolean) {
    return act(name, "pause", async () => {
      of(name).detail = await invoke<SyncDetail>("project_sync_pause", { project: name, paused });
    });
  }

  /// Syncs the folder now at `m`'s path from nothing; nothing is deleted elsewhere.
  function startOver(name: string, m: MachineDetail) {
    return act(name, `start-over:${m.machine_id}`, () => invoke("project_sync_start_over", { project: name, machineId: m.machine_id }));
  }

  function resolve(name: string, c: Conflict, keepCopy: boolean) {
    return act(name, c.path, () => invoke("project_sync_resolve", { project: name, machineId: c.resolver, path: c.path, keepCopy }));
  }

  /// Shows what differs on a file conflict, or hides it again.
  async function compare(name: string, c: Conflict, machines: readonly MachineDetail[]) {
    const key = `${name}\n${c.path}`;
    const existing = compared.get(key);

    if (existing) {
      if (existing.kind === "diff") existing.view.dispose();
      compared.delete(key);
      return redraw();
    }

    const token = {};
    compared.set(key, { kind: "reading", token });
    redraw();
    const machine = machines.find(m => m.machine_id === c.holder);
    const done = machine ? await compareFiles(invoke, machine, c) : "No machine that has it is online";
    const reading = compared.get(key);
    if (reading?.kind !== "reading" || reading.token !== token) return;

    if (typeof done === "string") {
      compared.set(key, { kind: "note", text: done });
    } else {
      const [{ windowDiffOwner }, { diffView }] = await Promise.all([import("../review/diff-owner.ts"), import("../review/diff.tsx")]);
      const after = compared.get(key);
      if (after?.kind !== "reading" || after.token !== token) return;
      const view = diffView({ owner: windowDiffOwner(), load: async () => done,
        review: { key: "compare", heading: () => "Review", send: async () => "Comments are not sent from here" } });
      compared.set(key, { kind: "diff", view });
    }

    redraw();
  }

  return {
    ensure,
    take,
    detail: (name: string) => of(name).detail,
    forTile,
    page: (name: string) => { const s = of(name); if (s.detail === undefined) void load(name); return { detail: s.detail, error: s.error, busy: s.busy }; },
    compared: (name: string, path: string) => {
      const entry = compared.get(`${name}\n${path}`);
      return !entry ? undefined : entry.kind === "reading" ? null : entry.kind === "note" ? entry.text : entry.view.el;
    },
    pause, startOver, resolve, compare,
    fix: deps.fix ? (name: string, conflict: Conflict) => deps.fix?.(name, conflict) : null,
    fixButton: deps.fixButton ?? null,
    repairButton: deps.repairButton ?? null,
  };
}

/// "2.1 MB/s", or "" before the rate is known.
export function rateText(rate: number | null | undefined) {
  return rate && rate > 0 ? `${bytes(rate)}/s` : "";
}

/// "pack-1a2b.pack: 12 MB of 40 MB" for the file a sync waits on, or "".
export function fileText(file: FileProgress | null) {
  return file ? `${file.path.slice(file.path.lastIndexOf("/") + 1)}: ${bytes(file.done)} of ${bytes(file.size)}` : "";
}

/// "under a minute", "about 1 min", "about 12 min", "about 2 h", or ""
/// before it can tell.
export function timeLeft(secs: number | null | undefined) {
  if (secs == null) return "";
  if (secs < 45) return "under a minute";
  if (secs < 90) return "about 1 min";
  if (secs < 3600) return `about ${Math.round(secs / 60)} min`;
  return `about ${Math.round(secs / 3600)} h`;
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
/// "offline", "waiting for Syncthing", "folder missing", "disk missing" or
/// "not syncing", and whether it asks for a look.
export function stateChip(m: Pick<MachineDetail, "status" | "paused" | "progress" | "files" | "pending" | "conflicts"> & Partial<Pick<MachineDetail, "error">>): { text: string; wait: boolean } {
  if (m.status === "offline") return { text: "offline", wait: false };
  if (syncedBySyncthing(m)) return { text: "waiting for Syncthing", wait: false };
  if (m.status === "missing") return { text: "folder missing", wait: true };
  if (m.status === "parent_missing") return { text: "disk missing", wait: true };
  if (m.paused) return { text: "paused", wait: false };
  if (m.status === "failed") return { text: gitSyncWarning({ status: m.status, error: m.error ?? null }) ?? "not syncing", wait: true };
  if (m.status === "scanning") return { text: "scanning", wait: false };
  if (m.status === "syncing") return { text: `syncing ${syncPct(m)}%`, wait: false };
  return { text: "in sync", wait: m.conflicts.length > 0 };
}

/// What a machine's row says under its name when it does not sync, with the
/// node's reason in the title: null while it syncs or says nothing.
export function stateNote(m: Pick<MachineDetail, "status" | "error" | "can_start_over">): string | null {
  if (m.can_start_over) return "This folder was replaced, so sync stopped.";
  if (m.status === "missing") return "The folder is gone. Beep makes it again and fills it from the other machines.";
  if (m.status === "parent_missing") return "The folder above it is gone too, so nothing is made. Is its disk mounted?";
  if (m.status === "syncthing") return "Syncthing keeps this folder in step. Beep syncs it once Syncthing pauses or lets go of it.";
  return m.status === "failed" ? m.error : null;
}

/// The header's word for the whole project: "in sync" only when every
/// machine is; else what holds it up.
export function syncSummary(d: Pick<SyncDetail, "pinned" | "paused" | "machines">) {
  if (d.pinned) return "pinned";
  if (d.paused) return "paused on every machine";
  const all = d.machines;
  if (!all.length) return "";
  const moving = all.filter(m => !m.paused && (m.status === "syncing" || m.status === "scanning")).length;
  if (moving) return `${moving} syncing`;
  // Machines Syncthing keeps in step are fine, just not Beep's to sync.
  const st = all.filter(syncedBySyncthing).length;
  const off = all.filter(m => m.status === "offline").length;
  if (st && st + off === all.length) return ["Syncthing keeps it in step", off && `${off} offline`].filter(Boolean).join(" · ");
  const ours = all.length - st;
  const synced = all.filter(m => m.status === "in_sync" && !m.paused).length;
  if (synced === ours) return st ? `in sync · ${st} by Syncthing` : "in sync";
  const stuck = ours - synced - off;
  return [`${synced} of ${ours} in sync`, stuck && `${stuck} not syncing`, off && `${off} offline`, st && `${st} by Syncthing`].filter(Boolean).join(" · ");
}

/// "1 change waiting", "3 changes waiting".
function changes(n: number) { return `${plural(n, "change", "changes")} waiting`; }

/// "1 conflict", "3 conflicts".
export function conflictCount(n: number) { return plural(n, "conflict", "conflicts"); }

/// The muted line under a machine: files and bytes, or how far it is with
/// the rate and time left, or what waits.
export function machineMeta(m: Pick<MachineDetail, "status" | "paused" | "progress" | "files" | "bytes" | "pending" | "waiting"> & Partial<Pick<MachineDetail, "error">>) {
  if (syncedBySyncthing(m)) return [];
  const p = m.progress;

  if (m.status === "syncing" && p && !m.paused) {
    return [
      `${p.files_done.toLocaleString("en-US")} of ${p.files_total.toLocaleString("en-US")} files`,
      p.bytes_total ? `${bytes(p.bytes_done)} of ${bytes(p.bytes_total)}` : "",
      rateText(p.rate),
      timeLeft(p.eta_secs),
      fileText(p.file),
    ].filter(Boolean);
  }

  const left = Math.max(m.pending, m.waiting.length);
  // An offline machine's counts are not known.
  if (m.status === "offline") return left ? [changes(left)] : [];
  const out = [`${m.files.toLocaleString("en-US")} ${m.files === 1 ? "file" : "files"}`];
  // A `project_sync` row has no size.
  if (m.bytes) out.push(bytes(m.bytes));
  if (left) out.push(changes(left));
  return out;
}

/// The file conflict copy `path` stands beside: `notes.txt` for
/// `notes (conflict 1a2b3c4d).txt`; the path itself when it is not one.
export function originalOf(path: string) {
  const git = gitConflict(path);
  if (git) return `.git/${git.original}`;
  const m = /^(.*) \(conflict [0-9a-f]{8}\)([^/]*)$/.exec(path);
  return m && !m[1].endsWith("/") && m[1] !== "" ? m[1] + m[2] : path;
}

/// The ref a git conflict `path` keeps the losing commit in, and the ref
/// it stands beside: `refs/beep-conflicts/heads/main-5e6f7a8b` and
/// `refs/heads/main`. Null for a file.
export function gitConflict(path: string) {
  const m = /^\.git\/refs\/beep-conflicts\/(.+)-[0-9a-f]{8}$/.exec(path);
  return m ? { ref: path.slice(".git/".length), original: `refs/${m[1]}` } : null;
}

/// The prompt that has an agent settle conflict `c`. A file is merged but not
/// committed; a git ref is kept until the user says to drop it.
export function conflictPrompt(c: Pick<Conflict, "path" | "original">) {
  const git = gitConflict(c.path);

  if (git) {
    return [
      `Two machines committed to ${git.original} at once, and file sync kept the other side as ${git.ref}.`,
      `Inspect it with \`git log\` and \`git diff\` against ${git.original} and say what differs. Merge it into the branch without rewriting history: no reset, rebase or force. Do not push.`,
      "If the two sides contradict, ask me which to keep before merging.",
      `Delete ${git.ref} with \`git update-ref -d\` only if I ask you to.`,
    ].join("\n");
  }

  return [
    `${c.original} was edited on two machines at once, and file sync kept the other side as ${c.path} beside it.`,
    `Read ${c.original} and ${c.path} and say what differs. Merge them into ${c.original}, keeping both sides' intent, then delete ${c.path}. Do not commit.`,
    "If the two contradict, ask me which to keep before changing anything.",
  ].join("\n");
}

/// Every conflict copy once, with the machines that have it and the first
/// online one that can settle it.
export function conflictList(d: Pick<SyncDetail, "machines"> | null | undefined): Conflict[] {
  const out = new Map<string, Conflict>();

  for (const m of d?.machines || []) {
    for (const path of m.conflicts) {
      let c = out.get(path);
      if (!c) out.set(path, c = { path, original: originalOf(path), on: [], resolver: null, holder: null });
      c.on.push(m.machine_id);
      if (!c.holder && m.status !== "offline") c.holder = m.machine_id;
      if (!c.resolver && m.can_resolve && m.status !== "offline") c.resolver = m.machine_id;
    }
  }

  return [...out.values()].sort((a, b) => a.path.localeCompare(b.path));
}

/// What a conflict row says under its path: the project, what the copy is
/// and the machines that have it.
export function conflictSub(name: string | null, c: Conflict, machines: readonly Pick<MachineDetail, "machine_id" | "machine_name">[]) {
  const where = c.on.map(id => machines.find(m => m.machine_id === id)?.machine_name || "").filter(Boolean).join(", ");
  const git = gitConflict(c.path);
  const what = git ? `Another commit of ${git.original.replace(/^refs\/heads\//, "")}` : `Another copy of ${c.original}`;
  return `${name ? `${name} · ` : ""}${what}${where ? ` · on ${where}` : ""}`;
}

/// The changes still waiting, each path once, with the machines it waits on.
export function waitingList(d: Pick<SyncDetail, "machines"> | null | undefined) {
  const out = new Map<string, string[]>();
  for (const m of d?.machines || []) for (const path of m.waiting) out.set(path, [...(out.get(path) || []), m.machine_name]);
  return [...out].map(([path, machines]) => ({ path, machines }));
}

type RowLike = Pick<SyncRow, "machine_id" | "status" | "files" | "pending"> & Partial<Pick<SyncRow, "error">>;

/// A machine's sync as far as its `project_sync` row tells.
export function rowDetail(r: RowLike, name = "Machine"): MachineDetail {
  return {
    machine_id: r.machine_id, machine_name: name, folder: "", status: r.status,
    files: r.files, bytes: 0, pending: r.pending, conflicts: [], error: r.error ?? null, hint: null, paused: false, progress: null, waiting: [], can_resolve: false, can_start_over: false,
  };
}

/// The board's File sync tile: one row per machine with its chip, from the
/// detail once it is read, else from the `project_sync` rows.
export function fileSyncTile(d: SyncDetail | null | undefined, rows?: readonly RowLike[] | null, names?: ReadonlyMap<string, string> | null): TileData {
  const machines: MachineDetail[] = d ? d.machines : (rows || []).map(r => rowDetail(r, names?.get(r.machine_id)));
  if (d === undefined && !machines.length) return { count: "", rows: [], empty: "Loading…" };
  const conflicts = conflictList({ machines }).length;
  const summary = syncSummary({ pinned: !!d?.pinned, paused: !!d?.paused, machines });
  const count = d?.paused ? "paused" : summary === "in sync" && conflicts ? conflictCount(conflicts) : summary;
  const out: TileRow[] = machines.map(m => {
    const chip = stateChip(m);
    return {
      id: m.machine_id,
      text: m.machine_name,
      dot: m.status === "offline" ? "off" : "",
      chip: chip.wait ? { cls: "wait", text: chip.text } : null,
      note: chip.wait ? "" : chip.text,
      title: m.folder ? `${m.machine_name}: ${plainPath(m.folder)}` : m.machine_name,
    };
  });
  return { count, rows: out, empty: d?.pinned ? "Pinned: it stays on the machines it is on" : "No machine syncs it" };
}

/// One machine's sync of one project, as `project_sync` lists it (SyncRow).
export interface SyncRow {
  project: string;
  machine_id: string;
  /// `offline`, `scanning`, `syncing`, `in_sync` or `failed`.
  status: string;
  files: number;
  pending: number;
  error: string | null;
}

/// New `project_sync` rows against the shown ones: the rows to keep, with
/// each unchanged row the same object, and the projects whose rows differ
/// (added, removed or changed on some machine). Nothing changed keeps
/// `before` itself.
export function mergeSyncRows(before: SyncRow[], after: SyncRow[]): { rows: SyncRow[]; changed: Set<string> } {
  const key = (r: SyncRow) => `${r.project}\n${r.machine_id}`;
  const old = new Map(before.map(r => [key(r), r]));
  const changed = new Set<string>();

  const rows = after.map(r => {
    const was = old.get(key(r));
    old.delete(key(r));
    if (was && JSON.stringify(was) === JSON.stringify(r)) return was;
    changed.add(r.project);
    return r;
  });

  for (const gone of old.values()) changed.add(gone.project);
  const reordered = rows.some((r, i) => r !== before[i]) || rows.length !== before.length;
  return { rows: reordered ? rows : before, changed };
}

/// A Windows folder as the user knows it, without the `\\?\` prefix a
/// resolved path carries; any other path as it is.
export function plainPath(path: string): string {
  if (path.startsWith("\\\\?\\UNC\\")) return "\\\\" + path.slice(8);
  if (/^\\\\\?\\[A-Za-z]:/.test(path)) return path.slice(4);
  return path;
}
