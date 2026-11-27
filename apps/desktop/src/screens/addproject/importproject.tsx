import { type ReactNode } from "react";
import { flushSync } from "react-dom";
import { ImportProjectView } from "./importproject-view.tsx";
import { pickFolder } from "./folderbrowser.tsx";
import { syncthingImportChoice } from "../project/syncthing.ts";
// The "Import a project" sheet: one field for org/repo or a URL, a chip per
// online machine (Auto lets Beep pick), the folder a clone goes into (by
// default <Projects in>/<org>/<repo>), then Import.
import { invoke } from "../../ui/tauri.ts";
import { openSheet } from "../../ui/parts.tsx";
import { START, next, looksLikeRepo, folderNote, goesInText, hasCustomFolder, keepLastImport, projectsIn, typedFolder } from "../../stores/addproject/importproject-state.ts";
export { keepLastImport };
import type { ImportEvent } from "../../stores/addproject/importproject-state.ts";
import type { MachineFolder, ProjectRow } from "../../stores/project/projects-state.ts";
import { defaultPlaces, worktreesToAsk } from "../../stores/review/branchwork.ts";
import type { BranchPicks, ImportFork, PreviewView } from "../../stores/review/branchwork.ts";
import { reason } from "../../ui/format.ts";
import type { MachineCard } from "../../parts/machines/machine-state.ts";

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
/// The folder field's text once the user types in it; null while it shows
/// the folder chosen or the default parent.
let folderText: string | null = null;
/// What `list_machine_folder` read for the typed folder, keyed by `machine\npath`.
let typedListing: MachineFolder | null = null, typedFor = "";
let typedTimer: ReturnType<typeof setTimeout> | undefined;
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
  if (options.folder) scheduleTypedFolder();
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

/// Syncthing is asked about the target folder chosen or typed, else the default parent.
function followSyncthing() {
  void syncthing.follow(state.machine, targetFolder() ?? parentFolder());
}

/// The folder chosen or typed, when not the machine's default parent.
function targetFolder() {
  if (!state.machine) return null;
  return state.chosen?.path ?? typedFolder(folderText, defaultParent());
}

/// The parent a clone goes under when no target folder was chosen or typed.
function parentFolder() {
  if (!state.machine || targetFolder()) return null;
  return defaultParent()?.trim() || null;
}

/// The listing read for the chosen or typed target folder, when current.
function activeListing() {
  if (state.chosen) return state.chosen;
  const target = targetFolder();
  return state.machine && target && typedFor === `${state.machine}\n${target}` ? typedListing : null;
}

/// Reads the typed folder and its Syncthing state once typing pauses.
function scheduleTypedFolder(immediate = false) {
  clearTimeout(typedTimer);
  const machine = state.machine, target = state.chosen ? null : targetFolder(), mine = opening;
  if (!machine || !target) {
    typedListing = null;
    typedFor = "";
    followSyncthing();
    return;
  }
  const key = `${machine}\n${target}`;
  if (typedFor !== key) {
    typedListing = null;
    typedFor = "";
  }
  const run = () => {
    if (mine !== opening || !root || state.machine !== machine || state.chosen || targetFolder() !== target) return;
    followSyncthing();
    if (typedFor === key) return;
    if (!target.startsWith("/") && target !== "~" && !target.startsWith("~/")) return;
    void invoke<MachineFolder>("list_machine_folder", { machineId: machine, path: target }).then(got => {
      if (mine !== opening || !root || state.machine !== machine || state.chosen || targetFolder() !== target) return;
      typedListing = got;
      typedFor = key;
      draw();
    }, () => {});
  };
  if (immediate) run();
  else typedTimer = setTimeout(run, 150);
}

async function submit() {
  const was = state;
  update({ type: "submit" });
  if (state === was) return;
  // The branches picked go with it only when they were read for this repo.
  const worktrees = preview && previewFor === state.repo.trim() ? worktreesToAsk(preview, picks) : [];
  const mine = opening, then = onDone;

  try {
    const project = await invoke<ProjectRow>("import_project", { repo: state.repo, machineId: state.machine, folder: targetFolder(), worktrees, parent: parentFolder(), syncthingFiles: syncthing.value() });
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
  const customFolder = hasCustomFolder(state, folderText, defaultParent());
  flushSync(() => current(<ImportProjectView
    state={state} machines={machines} preview={preview} previewFor={previewFor} previewError={previewError} fork={fork} picks={picks} folder={folder} goes={goesIn()} listing={activeListing()} customFolder={customFolder} syncChoice={syncthing}
    edit={(repo: string) => { update({ type: "edit", repo }); schedulePreview(); }}
    pick={(machine: string | null) => { update({ type: "pick", machine }); learnBase(); }}
    submit={() => void submit()} close={closeImportProject} choose={choose} unchoose={() => update({ type: "unchoose" })}
    folderInput={(value: string) => {
      if (state.chosen) state = next(state, { type: "unchoose" });
      folderText = value;
      scheduleTypedFolder();
      draw();
    }}
    pickBranch={(branch: string, machine: string | null) => { if (machine === null) delete picks[branch]; else picks[branch] = machine; draw(); }}
  />));
}

/// Learns the picked machine's Projects in folder, the default it shows.
function learnBase() {
  const machine = state.machine, mine = opening;
  if (!machine || roots.has(machine)) return;
  void projectsIn(invoke, machine).then(folder => {
    roots.set(machine, folder);
    if (mine === opening && root && state.machine === machine) { scheduleTypedFolder(true); draw(); }
  });
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
  void pickFolder({ machine, start: targetFolder() ?? parentFolder(), heading: "Choose a folder", noteFor: async listing => folderNote(listing, state.repo) }).then(listing => {
    if (listing && mine === opening && root && state.machine === machine) {
      update({ type: "choose", listing });
      schedulePreview();
    }
  });
}

/// The parent a clone goes under before anything is typed.
function defaultParent() {
  return state.machine ? roots.get(state.machine) ?? null : null;
}

/// Destination line under Clone into before a folder listing is read.
function goesIn() {
  return goesInText(state.repo, targetFolder(), parentFolder());
}

/// Forgets the folder typed.
function resetFolder() {
  clearTimeout(typedTimer);
  folderText = null;
  typedListing = null;
  typedFor = "";
}
