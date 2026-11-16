import { type ReactNode } from "react";
import { flushSync } from "react-dom";
import { ImportProjectView } from "./importproject-view.tsx";
import { pickFolder } from "./folderbrowser.tsx";
import { syncthingImportChoice } from "../project/syncthing.ts";
// The "Import a project" sheet: one field for org/repo or a URL, a chip per
// online machine (Auto lets Beep pick), the parent folder a clone goes
// under as <parent>/<org>/<repo> (the machine's Projects in folder), then Import.
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.tsx";
import { START, next, looksLikeRepo, folderNote, folderUnder, type LastImport } from "./importproject-state.ts";
import type { ImportEvent } from "./importproject-state.ts";
import type { ProjectRow } from "../project/projects-state.ts";
import { defaultPlaces, worktreesToAsk } from "../review/branchwork.ts";
import type { BranchPicks, ImportFork, PreviewView } from "../review/branchwork.ts";
import { reason } from "../ui/format.ts";
import type { MachineCard } from "../machines/machine-state.ts";

/// A machine the sheet offers, as Home lists it.
export type Machine = Pick<MachineCard, "id" | "name" | "online" | "env" | "can_host">;

/// What the window hands the sheet.
interface ImportProjectOptions {
  /// The machines Home lists.
  machines?: readonly Machine[];
  /// This Mac's machine id, when it is one.
  macId?: string | null;
  /// Runs once a project is in, as when the window selects it.
  onDone?: ((project: ProjectRow) => void) | null;
  /// Fills in the repository, as Clone here on a project's page does.
  repo?: string | null;
  /// Picks the machine, as Clone here does.
  machine?: string | null;
  /// Fills in the folder it goes in.
  folder?: string | null;
}

const LAST_KEY = "beep.lastImport.v1";

export function lastImport(): LastImport | null {
  try {
    const v = JSON.parse(localStorage.getItem(LAST_KEY) || "null");
    return v && typeof v.machine === "string" ? { machine: v.machine } : null;
  } catch {
    return null;
  }
}

/// Notes the machine `project` landed on, for the next import.
export function keepLastImport(project: ProjectRow) {
  const last: LastImport = { machine: project.machine_id };
  try { localStorage.setItem(LAST_KEY, JSON.stringify(last)); } catch { /* kept for this launch only */ }
}

let syncthing = syncthingImportChoice();
let root: HTMLDivElement | null = null, state = START, machines: readonly Machine[] = [];
let macId: string | null = null, close: (() => void) | null = null, onDone: ((project: ProjectRow) => void) | null = null;
/// The repository's checkouts and branches with work, once read: for the repo
/// text `previewFor`, null when it could not be read.
let preview: PreviewView | null | undefined = undefined, previewFor = "", previewError = "", picks: BranchPicks = {};
let previewTimer: ReturnType<typeof setTimeout> | undefined;
/// The user's fork the import would clone, for the repo text `previewFor`:
/// undefined while it is read, null when it could not be read.
let fork: ImportFork | null | undefined = undefined;
/// Each machine's Projects in folder, by id, once read.
const roots = new Map<string, string>();
/// The parent field's text once the user types in it; null while it shows
/// the folder chosen or the default parent.
let folderText: string | null = null;
let view: ((content: ReactNode) => void) | null = null, opening = 0;

/// Opens the sheet.
export function openImportProject(options: ImportProjectOptions = {}) {
  if (root) return;
  opening++;
  syncthing = syncthingImportChoice();
  machines = (options.machines || []).filter(m => m.can_host !== false);
  macId = options.macId || null;
  onDone = options.onDone || null;
  // Start on this computer when it is one of the machines.
  state = macId && machines.some(m => m.id === macId) ? { ...START, machine: macId } : START;
  if (options.machine && machines.some(m => m.id === options.machine)) state = { ...state, machine: options.machine };
  if (options.repo) state = { ...state, repo: options.repo };
  ({ root, close, render: view } = openSheet("Import a project", "am-sheet ip-sheet", {
    canClose: () => state.step !== "cloning",
    onClose: closed,
  }));
  preview = undefined;
  fork = undefined;
  previewFor = "";
  previewError = "";
  picks = {};
  resetFolder();
  if (options.folder) folderText = options.folder;
  roots.clear();
  draw();
  learnBase();
  schedulePreview();
}

/// Reads the preview for the repository typed, once typing pauses.
function schedulePreview() {
  clearTimeout(previewTimer);
  const repo = state.repo.trim(), mine = opening;
  if (!looksLikeRepo(repo) || repo === previewFor) return;
  previewTimer = setTimeout(async () => {
    previewFor = repo;
    preview = undefined;
    previewError = "";
    picks = {};
    fork = undefined;
    if (root) draw();
    invoke<ImportFork>("find_import_fork", { repo }).then(got => got, () => null).then(got => {
      if (mine !== opening || !root || previewFor !== repo) return;
      fork = got;
      if (root && state.step === "form") draw();
    });

    try {
      const got = await invoke<PreviewView>("preview_import", { repo });
      if (mine !== opening || !root || previewFor !== repo) return;
      preview = got;
      picks = defaultPlaces(got);
    } catch (e) {
      if (mine !== opening || !root || previewFor !== repo) return;
      preview = null;
      previewError = reason(e);
    }

    if (mine === opening && root && state.step !== "cloning" && state.step !== "done") draw();
  }, 700);
}

function closeImportProject() {
  close?.();
}

function closed() {
  syncthing.dispose();
  view = null;
  root = close = null;
  const done = state.step === "done" ? state.project : null;
  state = START;
  clearTimeout(previewTimer);
  preview = undefined;
  previewFor = "";
  resetFolder();
  const then = onDone;
  onDone = null;
  if (done && then) then(done);
}

function update(event: ImportEvent) {
  // Picking a machine or a folder replaces whatever was typed as the folder.
  if (event.type === "pick" || event.type === "choose" || event.type === "unchoose") resetFolder();
  state = next(state, event);
  followSyncthing();
  if (root) draw();
}

/// Syncthing is asked about the folder chosen, else the parent a clone goes under.
function followSyncthing() {
  void syncthing.follow(state.machine, state.chosen?.path ?? parentFolder());
}

/// The parent a clone goes under: the one typed, else the default; null on
/// Auto or with a folder chosen.
function parentFolder() {
  if (!state.machine || state.chosen) return null;
  return (folderText ?? defaultParent())?.trim() || null;
}

async function submit() {
  const was = state;
  update({ type: "submit" });
  if (state === was) return;
  // The branches picked go with it only when they were read for this repo.
  const worktrees = preview && previewFor === state.repo.trim() ? worktreesToAsk(preview, picks) : [];
  const mine = opening, then = onDone;

  try {
    const project = await invoke<ProjectRow>("import_project", { repo: state.repo, machineId: state.machine, folder: state.chosen?.path ?? null, worktrees, parent: parentFolder(), syncthingFiles: syncthing.value() });
    keepLastImport(project);
    if (mine !== opening || !root) { then?.(project); return; }
    update({ type: "done", project });
  } catch (e) {
    if (mine === opening && root) update({ type: "failed", message: String(e) });
    else console.warn("importing a project:", e);
  }
}

function draw() {
  if (!view) return;
  const current = view;
  const folder = state.machine ? folderText ?? state.chosen?.path ?? defaultParent() ?? "" : "";
  flushSync(() => current(<ImportProjectView
    state={state} machines={machines} preview={preview} previewFor={previewFor} previewError={previewError} fork={fork} picks={picks} folder={folder} goes={goesIn()} syncChoice={syncthing}
    edit={(repo: string) => { update({ type: "edit", repo }); schedulePreview(); }}
    pick={(machine: string | null) => { update({ type: "pick", machine }); learnBase(); }}
    submit={() => void submit()} close={closeImportProject} choose={choose} unchoose={() => update({ type: "unchoose" })}
    folderInput={(value: string) => {
      if (state.chosen) { resetFolder(); state = next(state, { type: "unchoose" }); followSyncthing(); }
      folderText = value; draw();
    }}
    pickBranch={(branch: string, machine: string | null) => { if (machine === null) delete picks[branch]; else picks[branch] = machine; draw(); }}
  />));
}

/// Learns the picked machine's Projects in folder, the default it shows.
function learnBase() {
  const machine = state.machine, mine = opening;
  if (!machine || roots.has(machine)) return;
  void projectsIn(machine).then(folder => {
    roots.set(machine, folder);
    if (mine === opening && root && state.machine === machine) { followSyncthing(); draw(); }
  });
}

/// Where `machine` keeps its projects: its Projects in folder, else its home.
export function projectsIn(machine: string): Promise<string> {
  return invoke<string>("machine_search", { machineId: machine }).then(folder => folder.trim() || "~", () => "~");
}

/// Choose…: the picker of the machine's folders, opened where the folder shown is.
function choose() {
  // On Auto, Choose… picks this computer, or else the first machine listed.
  if (!state.machine) {
    const id = macId && machines.some(m => m.id === macId) ? macId : machines[0]?.id;
    if (!id) return;
    update({ type: "pick", machine: id });
    learnBase();
  }

  const machine = state.machine, mine = opening;
  if (!machine) return;
  void pickFolder({ machine, start: state.chosen?.path ?? parentFolder(), heading: "Choose a folder", noteFor: async listing => folderNote(listing, state.repo) }).then(listing => {
    if (listing && mine === opening && root && state.machine === machine) update({ type: "choose", listing });
  });
}

/// The parent a clone goes under before anything is typed.
function defaultParent() {
  return state.machine ? roots.get(state.machine) ?? null : null;
}

/// "Goes in <parent>/<org>/<repo>." under the parent field.
function goesIn() {
  const parent = parentFolder();
  if (!parent) return "";
  return `Goes in ${folderUnder(parent, state.repo) ?? `${parent.replace(/\/+$/, "")}/<org>/<repo>`}.`;
}

/// Forgets the folder typed.
function resetFolder() {
  folderText = null;
}
