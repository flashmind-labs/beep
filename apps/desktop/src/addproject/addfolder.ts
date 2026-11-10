import { createElement, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { AddFolderView } from "./addfolder-view.tsx";
import { syncthingImportChoice } from "../project/syncthing.ts";
// The "Add a folder" sheet: a machine (this Mac first), a folder on it, what
// that folder is (a checkout, its remote and branch), then Add project, which
// adds the checkout in place with no clone. Choose… opens the macOS folder
// picker on this Mac, only when clicked, and the folder browser elsewhere.
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.ts";
import { keepLastImport } from "./importproject.ts";
import { chooseWith, nextFolder, repoOf, startOn } from "./addfolder-state.ts";
import type { FolderEvent, FolderState } from "./addfolder-state.ts";
import type { MachineFolder, ProjectRow } from "../project/projects-state.ts";

/// What the window hands the sheet.
export interface AddFolderOptions {
  /// The machines Home lists.
  machines?: readonly { id: string; name: string; online: boolean; can_host?: boolean }[];
  /// This Mac's machine id, when it is one.
  macId?: string | null;
  /// Runs once the project is in.
  onDone?: ((project: ProjectRow) => void) | null;
  /// Back to the Add a project sheet.
  onBack?: (() => void) | null;
}

let syncthing = syncthingImportChoice();
let root: HTMLDivElement | null = null, close: (() => void) | null = null;
let opts: AddFolderOptions = {};
let state: FolderState = startOn([], null);
/// A folder is being read, so the folder buttons wait.
let reading = false;
let view: ((content: ReactNode) => void) | null = null, opening = 0, branchRequest = 0;

/// Opens the sheet on this Mac. Nothing opens the folder picker until Choose….
export function openAddFolder(options: AddFolderOptions = {}) {
  if (root) return;
  opening++;
  syncthing = syncthingImportChoice();
  opts = { ...options, machines: options.machines?.filter(m => m.can_host !== false) };
  state = startOn(opts.machines || [], opts.macId || null);
  reading = false;
  ({ root, close, render: view } = openSheet("Add a folder", "am-sheet ip-sheet", {
    canClose: () => state.step !== "adding",
    onClose: closed,
  }));
  draw();
}

function closed() {
  syncthing.dispose();
  view = null;
  root = close = null;
  const done = state.step === "done" ? state.project : null;
  const then = opts.onDone;
  opts = {};
  if (done && then) then(done);
}

function update(event: FolderEvent) {
  if (event.type === "pick" || event.type === "choose") branchRequest++;
  state = nextFolder(state, event);
  void syncthing.follow(state.machine, state.chosen?.path ?? null);
  if (root) draw();
}

/// Reads `path` (null for where the machine starts) on the picked machine.
async function read(path: string | null, then: (listing: MachineFolder) => void) {
  const machine = state.machine, mine = opening;
  if (!machine || reading) return;
  reading = true;
  draw();

  try {
    const listing = await invoke<MachineFolder>("list_machine_folder", { machineId: machine, path });
    if (mine === opening && root && state.machine === machine) then(listing);
  } catch (e) {
    if (mine === opening && root && state.machine === machine) update({ type: "oops", message: String(e) });
  } finally {
    if (mine === opening && root) { reading = false; draw(); }
  }
}

/// Picks `listing` and reads its branch when it is a checkout.
function use(listing: MachineFolder) {
  update({ type: "choose", listing });
  const machine = state.machine, mine = opening, request = branchRequest;
  if (listing.origin == null || !machine) return;
  invoke<string | null>("folder_branch", { machineId: machine, path: listing.path })
    .then(branch => branch, () => null)
    .then(branch => { if (mine === opening && request === branchRequest && root && state.machine === machine) update({ type: "branch", path: listing.path, branch }); });
}

const browseTo = (path: string | null) => read(path, listing => update({ type: "browse", listing }));

/// Choose…: the macOS folder picker on this Mac, the folder browser elsewhere.
async function choose() {
  if (reading) return;
  const how = chooseWith(state.machine, opts.macId || null);

  if (how === "browser") {
    browseTo(state.chosen?.path ?? null);
    return;
  }

  if (how !== "picker") return;
  const mine = opening, machine = state.machine;
  let path: string | null = null;

  try {
    path = await invoke<string | null>("pick_folder", { start: state.chosen?.path ?? null });
  } catch (e) {
    if (mine === opening && root && state.machine === machine) update({ type: "oops", message: String(e) });
  }

  if (path && mine === opening && root && state.machine === machine) read(path, use);
}

async function submit() {
  const was = state;
  update({ type: "submit" });
  if (state === was || !state.chosen) return;

  const mine = opening, then = opts.onDone;
  try {
    const project = await invoke<ProjectRow>("import_project", {
      repo: repoOf(state.chosen), machineId: state.machine, folder: state.chosen.path, worktrees: [], parent: null, syncthingFiles: syncthing.value(),
    });
    keepLastImport(project);
    if (mine !== opening || !root) { then?.(project); return; }
    update({ type: "done", project });
  } catch (e) {
    if (mine === opening && root) update({ type: "failed", message: String(e) });
  }
}

function draw() {
  if (!view) return;
  const current = view;
  flushSync(() => current(createElement(AddFolderView, {
    state, machines: opts.machines ?? [], reading, sync: syncthing,
    pick: (machine: string) => update({ type: "pick", machine }), choose: () => void choose(), browse: browseTo,
    cancelBrowse: () => update({ type: "browse", listing: null }), use, submit: () => void submit(),
    back: () => { const back = opts.onBack; close?.(); back?.(); }, close: () => close?.(),
  })));
}
