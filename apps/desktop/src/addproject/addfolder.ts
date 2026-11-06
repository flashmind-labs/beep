// The "Add a folder" sheet: a machine (this Mac first), a folder on it, what
// that folder is (a checkout, its remote and branch), then Add project, which
// adds the checkout in place with no clone. Choose… opens the macOS folder
// picker on this Mac, only when clicked, and the folder browser elsewhere.
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.ts";
import { button, el, present, press } from "../ui/dom.ts";
import { folderBrowser } from "./folderbrowser.ts";
import { keepLastImport } from "./importproject.ts";
import { addable, chooseWith, folderFacts, nextFolder, repoOf, startOn } from "./addfolder-state.ts";
import type { FolderEvent, FolderState } from "./addfolder-state.ts";
import type { MachineFolder, ProjectRow } from "../project/projects-state.ts";

/// What the window hands the sheet.
export interface AddFolderOptions {
  /// The machines Home lists.
  machines?: readonly { id: string; name: string; online: boolean }[];
  /// This Mac's machine id, when it is one.
  macId?: string | null;
  /// Runs once the project is in.
  onDone?: ((project: ProjectRow) => void) | null;
  /// Back to the Add a project sheet.
  onBack?: (() => void) | null;
}

let root: HTMLDivElement | null = null, sheet: HTMLDivElement | null = null, close: (() => void) | null = null;
let opts: AddFolderOptions = {};
let state: FolderState = startOn([], null);
/// A folder is being read, so the folder buttons wait.
let reading = false;

/// Opens the sheet on this Mac. Nothing opens the folder picker until Choose….
export function openAddFolder(options: AddFolderOptions = {}) {
  if (root) return;
  opts = options;
  state = startOn(options.machines || [], options.macId || null);
  reading = false;
  ({ root, sheet, close } = openSheet("Add a folder", "am-sheet ip-sheet", {
    canClose: () => state.step !== "adding",
    onClose: closed,
  }));
  draw();
}

function closed() {
  root = sheet = close = null;
  const done = state.step === "done" ? state.project : null;
  const then = opts.onDone;
  opts = {};
  if (done && then) then(done);
}

function update(event: FolderEvent) {
  state = nextFolder(state, event);
  if (root) draw();
}

/// Reads `path` (null for where the machine starts) on the picked machine.
async function read(path: string | null, then: (listing: MachineFolder) => void) {
  const machine = state.machine;
  if (!machine || reading) return;
  reading = true;
  draw();
  try {
    const listing = await invoke<MachineFolder>("list_machine_folder", { machineId: machine, path });
    if (state.machine === machine) then(listing);
  } catch (e) {
    if (state.machine === machine) update({ type: "oops", message: String(e) });
  } finally {
    reading = false;
    if (root) draw();
  }
}

/// Picks `listing` and reads its branch when it is a checkout.
function use(listing: MachineFolder) {
  update({ type: "choose", listing });
  const machine = state.machine;
  if (listing.origin == null || !machine) return;
  invoke<string | null>("folder_branch", { machineId: machine, path: listing.path })
    .then(branch => branch, () => null)
    .then(branch => update({ type: "branch", path: listing.path, branch }));
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
  let path: string | null = null;
  try {
    path = await invoke<string | null>("pick_folder", { start: state.chosen?.path ?? null });
  } catch (e) {
    update({ type: "oops", message: String(e) });
  }
  if (path) read(path, use);
}

async function submit() {
  const was = state;
  update({ type: "submit" });
  if (state === was || !state.chosen) return;
  try {
    const project = await invoke<ProjectRow>("import_project", {
      repo: repoOf(state.chosen), machineId: state.machine, folder: state.chosen.path, worktrees: [], parent: null,
    });
    keepLastImport(project);
    update({ type: "done", project });
    // The sheet may have closed meanwhile: the project still lands.
    if (!root && opts.onDone) { const then = opts.onDone; opts = {}; then(project); }
  } catch (e) {
    update({ type: "failed", message: String(e) });
  }
}

function draw() {
  if (!sheet) return;
  sheet.replaceChildren(el("div", "am-head", el("b", "", "Add a folder"), el("kbd", "", "esc")), ...present(state.step === "done" ? done() : form()));
}

function done() {
  const project = state.project!;
  return [
    el("div", "am-done", el("span", "ddot"), el("b", "", `${project.name} added`)),
    el("div", "am-sub", `In place on ${project.machine_name} in ${project.folder}`),
    button("Open project", "dbtn pri", () => close?.()),
  ];
}

function form() {
  const machines = (opts.machines || []).filter(m => m.online);
  const chips = el("div", "dchips", ...machines.map(m =>
    press(button(m.name, "dchip", () => update({ type: "pick", machine: m.id })), m.id === state.machine)));
  const path = el("div", state.chosen ? "ip-path" : "ip-path dim", state.chosen?.path ?? "No folder chosen");
  path.title = state.chosen?.path ?? "";
  const pick = button(reading ? "Reading…" : "Choose…", "dbtn quiet af-choose", choose);
  pick.disabled = !state.machine || reading || state.step !== "form";
  const folder = state.browsing
    ? folderBrowser(state.browsing, {
      reading,
      note: folderFacts(state.browsing, null)[0] ?? null,
      go: browseTo,
      cancel: () => update({ type: "browse", listing: null }),
      use: () => state.browsing && use(state.browsing),
    })
    : el("div", "am-line", path, pick);
  const facts = folderFacts(state.chosen, state.branch);
  const go = button(state.step === "adding" ? "Adding…" : "Add project", "dbtn pri af-go", submit);
  go.disabled = state.step !== "form" || !addable(state);
  return [
    el("div", "am-label", "Machine"),
    el("div", "am-line", machines.length ? chips : el("span", "am-sub", "No machine is online.")),
    el("div", "am-label", "Folder"),
    folder,
    facts.length ? el("div", "ip-found", state.chosen?.origin && el("span", "ddot"), facts[0]) : null,
    facts[1] ? el("div", "am-sub", facts[1]) : null,
    state.error ? el("div", "am-msg bad", state.error) : null,
    el("div", "am-line ip-foot", go, button("Back", "dbtn quiet", () => { const back = opts.onBack; close?.(); back?.(); })),
  ];
}
