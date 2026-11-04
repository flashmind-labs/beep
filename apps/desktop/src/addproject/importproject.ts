// The "Import a project" sheet: one field for org/repo or a URL, a chip per
// online machine (Auto lets the server pick), the parent folder a clone goes
// under as <parent>/<org>/<repo> (the last one used there), then Import.
import { invoke } from "../ui/tauri.ts";
import { focusBack, linkSheetStyle, sheetShell } from "../machines/addmachine.ts";
import { START, importRoot, next, looksLikeRepo, machineChips, progressText, doneText, present, folderUnder, folderNote, folderHelp, reason } from "./importproject-state.ts";
import { button, el } from "../ui/dom.ts";
import type { ImportEvent } from "./importproject-state.ts";
import type { MachineFolder, ProjectRow } from "../project/projects-state.ts";
import { checkoutNote, defaultPlaces, forkNote, placeChoices, previewLine, worktreesToAsk } from "../review/branchwork.ts";
import type { BranchPicks, ImportFork, PreviewView } from "../review/branchwork.ts";
import type { MachineCard } from "../center/home-state.ts";
import { NO_MENU, completed, isOpen, menuKey, menuOf, splitTyped, suggest } from "./foldercomplete.ts";
import type { FolderMenu } from "./foldercomplete.ts";

/// A machine the sheet offers, as Home lists it.
type Machine = Pick<MachineCard, "id" | "name" | "online" | "env">;

/// What the window hands the sheet.
export interface ImportProjectOptions {
  /// The machines Home lists.
  machines?: readonly Machine[];
  /// This Mac's machine id, when it is one.
  macId?: string | null;
  /// Runs once a project is in, as when the window selects it.
  onDone?: ((project: ProjectRow) => void) | null;
  /// Goes straight to choosing a folder.
  choose?: boolean;
  /// Fills in the repository, as Clone here on a project's page does.
  repo?: string | null;
  /// Picks the machine, as Clone here does.
  machine?: string | null;
  /// Fills in the folder it goes in.
  folder?: string | null;
}

/// The machine and the folder projects went under at the last import, kept
/// across launches so the next import starts there.
export interface LastImport {
  machine: string;
  root: string | null;
}

const LAST_KEY = "beep.lastImport.v1";

export function lastImport(): LastImport | null {
  try {
    const v = JSON.parse(localStorage.getItem(LAST_KEY) || "null");
    return v && typeof v.machine === "string" ? { machine: v.machine, root: typeof v.root === "string" ? v.root : null } : null;
  } catch {
    return null;
  }
}

/// Notes where `project` landed, for the next import.
export function keepLastImport(project: ProjectRow) {
  const last: LastImport = { machine: project.machine_id, root: importRoot(project.folder, project.name) };
  try { localStorage.setItem(LAST_KEY, JSON.stringify(last)); } catch { /* kept for this launch only */ }
}

let root: HTMLDivElement | null = null, sheet: HTMLDivElement | null = null, state = START, machines: readonly Machine[] = [];
let macId: string | null = null, back: Element | null = null, onDone: ((project: ProjectRow) => void) | null = null;
/// The repository's checkouts and branches with work, once read: for the repo
/// text `previewFor`, null from a server too old to say.
let preview: PreviewView | null | undefined = undefined, previewFor = "", previewError = "", picks: BranchPicks = {};
let previewTimer: ReturnType<typeof setTimeout> | undefined;
/// The user's fork the import would clone, for the repo text `previewFor`:
/// undefined while it is read, null for none or a server too old to say.
let fork: ImportFork | null | undefined = undefined;
/// The folder each machine starts in (workspace, else home), by id, once read.
const bases = new Map<string, string>();
/// A folder is being read, so the folder buttons wait.
let reading = false;
/// The parent field's text once the user types in it; null while it shows
/// the folder chosen or the default parent.
let folderText: string | null = null;
/// The folders offered under the Folder field.
let menu: FolderMenu = NO_MENU;
/// The latest folder lookup: an answer to an older one is dropped.
let lookup = 0;
let lookupTimer: ReturnType<typeof setTimeout> | undefined;
/// The folders read while the sheet is open, by machine and folder.
const listed = new Map<string, readonly string[]>();

/// Opens the sheet. With `options.choose` it goes straight to choosing a
/// folder.
export function openImportProject(options: ImportProjectOptions = {}) {
  if (root) return;
  machines = options.machines || [];
  macId = options.macId || null;
  onDone = options.onDone || null;
  back = document.activeElement;
  linkSheetStyle();
  injectStyle();
  // Start on this Mac when it is one of the machines: Choose… then opens the
  // macOS folder picker straight away.
  state = macId && machines.some(m => m.id === macId) ? { ...START, machine: macId } : START;
  if (options.machine && machines.some(m => m.id === options.machine)) state = { ...state, machine: options.machine };
  if (options.repo) state = { ...state, repo: options.repo };
  ({ root, sheet } = sheetShell("Import a project", "am-sheet ip-sheet"));
  root.addEventListener("mousedown", e => { if (e.target === root && state.step !== "cloning") closeImportProject(); });
  document.body.append(root);
  document.addEventListener("keydown", onKey, true);
  preview = undefined;
  fork = undefined;
  previewFor = "";
  previewError = "";
  picks = {};
  resetFolder();
  if (options.folder) folderText = options.folder;
  listed.clear();
  draw();
  learnBase();
  schedulePreview();
  if (options.choose) choose();
}

/// Reads the preview for the repository typed, once typing pauses.
function schedulePreview() {
  clearTimeout(previewTimer);
  const repo = state.repo.trim();
  if (!looksLikeRepo(repo) || repo === previewFor) return;
  previewTimer = setTimeout(async () => {
    previewFor = repo;
    preview = undefined;
    previewError = "";
    picks = {};
    fork = undefined;
    if (root) draw();
    invoke<ImportFork | null>("find_import_fork", { repo }).then(got => got, () => null).then(got => {
      if (previewFor !== repo) return;
      fork = got;
      if (root && state.step === "form") draw();
    });
    try {
      const got = await invoke<PreviewView | null>("preview_import", { repo });
      if (previewFor !== repo) return;
      preview = got;
      picks = defaultPlaces(got);
    } catch (e) {
      if (previewFor !== repo) return;
      preview = null;
      previewError = reason(e);
    }
    if (root && state.step !== "cloning" && state.step !== "done") draw();
  }, 700);
}

export function closeImportProject() {
  if (!root) return;
  document.removeEventListener("keydown", onKey, true);
  root.remove();
  root = sheet = null;
  const done = state.step === "done" ? state.project : null;
  state = START;
  clearTimeout(previewTimer);
  preview = undefined;
  previewFor = "";
  resetFolder();
  listed.clear();
  focusBack(back);
  const then = onDone;
  onDone = null;
  if (done && then) then(done);
}

function onKey(e: KeyboardEvent) {
  if (e.key !== "Escape") return;
  e.preventDefault();
  e.stopPropagation();
  // Esc first closes the folder menu, then the sheet.
  if (isOpen(menu)) setMenu(NO_MENU);
  else closeImportProject();
}

function update(event: ImportEvent) {
  // Picking a machine or a folder replaces whatever was typed as the folder.
  if (event.type === "pick" || event.type === "choose" || event.type === "unchoose") resetFolder();
  state = next(state, event);
  if (root && event.type !== "edit") draw();
  else if (root) syncForm();
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
  try {
    const project = await invoke<ProjectRow>("import_project", { repo: state.repo, machineId: state.machine, folder: state.chosen?.path ?? null, worktrees, parent: parentFolder() });
    keepLastImport(project);
    update({ type: "done", project });
    // The sheet may have closed while cloning: the project still lands.
    if (!root && onDone) { const then = onDone; onDone = null; then(project); }
  } catch (e) {
    update({ type: "failed", message: String(e) });
  }
}

/// Keeps the typed text and focus, only enabling Import and clearing an error.
function syncForm() {
  const go = sheet!.querySelector<HTMLButtonElement>(".ip-go");
  if (go) go.disabled = !looksLikeRepo(state.repo);
  if (!state.error) sheet!.querySelector(".am-msg.bad")?.remove();
  // Where it goes follows the repository typed.
  const to = sheet!.querySelector(".ip-to");
  if (to) to.textContent = goesIn();
}

function draw() {
  const box = sheet!;
  // A redraw while typing a folder keeps the caret there.
  const was = document.activeElement;
  const typing = was instanceof HTMLInputElement && was.classList.contains("ip-dir") ? was.selectionStart ?? was.value.length : null;
  const keep = menu;
  box.replaceChildren(el("div", "am-head", el("b", "", "Import a project"), el("kbd", "", "esc")), ...present(body()));
  const field = box.querySelector("input");
  const pick = box.querySelector<HTMLElement>(".ip-browse .dirs button");
  const dir = box.querySelector<HTMLInputElement>(".ip-dir");
  if (typing != null && dir && !dir.disabled) { dir.focus(); dir.setSelectionRange(typing, typing); }
  else if (state.browsing && pick) pick.focus();
  else if (field && !state.browsing) { field.focus(); field.setSelectionRange(field.value.length, field.value.length); }
  else box.querySelector<HTMLElement>(".dbtn.pri")?.focus();
  // The menu is drawn again when the field kept focus, else dropped. The
  // old field's blur may have dropped it and a lookup on its way: ask again.
  setMenu(keep);
  if (typing != null && !isOpen(keep) && folderText != null) lookFolders();
}

/// Reads a folder on the picked machine (`path` null for where it starts),
/// then hands it to `then`; a failure shows as the sheet's error.
async function read(path: string | null, then: (listing: MachineFolder) => void) {
  const machine = state.machine;
  if (!machine || reading) return;
  reading = true;
  if (root) draw();
  try {
    const listing = await invoke<MachineFolder>("list_machine_folder", { machineId: machine, path });
    if (!path && !bases.has(machine)) bases.set(machine, listing.path);
    if (state.machine === machine) then(listing);
  } catch (e) {
    if (state.machine === machine) update({ type: "oops", message: String(e) });
  } finally {
    reading = false;
    if (root) draw();
  }
}

/// Learns where the picked machine starts, for the default folder it shows.
function learnBase() {
  if (state.machine && !bases.has(state.machine)) read(null, () => {});
}

/// Moves the folder browser to `path`.
const browseTo = (path: string | null) => read(path, listing => update({ type: "browse", listing }));

/// Choose…: the macOS picker on this Mac, the folder browser elsewhere.
async function choose() {
  if (reading) return;
  // On Auto, Choose… picks this Mac, or else the first machine listed.
  if (!state.machine) {
    const id = macId && machines.some(m => m.id === macId) ? macId : machines[0]?.id;
    if (!id) return;
    update({ type: "pick", machine: id });
  }
  if (state.machine !== macId) {
    browseTo(state.chosen?.path ?? null);
    return;
  }
  let path = null;
  try {
    path = await invoke<string | null>("pick_folder", { start: state.chosen?.path ?? bases.get(state.machine!) ?? null });
  } catch (e) {
    update({ type: "oops", message: String(e) });
  }
  if (path) read(path, listing => update({ type: "choose", listing }));
}

function body() {
  switch (state.step) {
    case "cloning":
      return [
        el("div", "am-wait", el("span", "dspin"), progressText(state, machines)),
        el("div", "am-sub", "A large repository takes a few minutes. You can close this; the project shows up once it is in."),
        el("div", "am-line", el("span", "end"), button("Close", "dbtn quiet", closeImportProject)),
      ];
    case "done": {
      const project = state.project!;
      return [
        el("div", "am-done", el("span", "ddot"), el("b", "", `${project.name} imported`)),
        el("div", "am-sub", doneText(project)),
        button("Open project", "dbtn pri", closeImportProject),
      ];
    }
    default:
      return form();
  }
}

function form() {
  const input = el("input", "ip-field");
  input.type = "text";
  input.placeholder = "org/repo or https://github.com/org/repo";
  input.spellcheck = false;
  input.autocapitalize = "off";
  input.setAttribute("autocorrect", "off");
  input.setAttribute("aria-label", "Repository");
  input.value = state.repo;
  input.addEventListener("input", () => { update({ type: "edit", repo: input.value }); schedulePreview(); });
  input.addEventListener("keydown", e => { if (e.key === "Enter") { e.preventDefault(); submit(); } });
  const chips = el("div", "dchips", ...machineChips(machines).map(c => {
    const b = button(c.label, "dchip", () => { update({ type: "pick", machine: c.id }); learnBase(); });
    b.setAttribute("aria-pressed", String(c.id === state.machine));
    return b;
  }));
  const go = button("Import", "dbtn pri ip-go", submit);
  go.disabled = !looksLikeRepo(state.repo);
  return [
    el("div", "am-label", "Repository"),
    el("div", "am-line", input),
    el("div", "am-label", "Machine"),
    el("div", "am-line", chips),
    el("div", "am-label", state.chosen ? "Folder" : "Clone into"),
    state.browsing ? browser(state.browsing) : folderLine(),
    state.chosen && !state.browsing ? el("div", "ip-found", state.chosen.origin != null && el("span", "ddot"), folderNote(state.chosen, state.repo)) : null,
    state.chosen || state.browsing ? null : el("div", "am-sub ip-to", goesIn()),
    forkLine(),
    el("div", "am-sub", folderHelp(state, machines)),
    ...branchesBlock(),
    state.error ? el("div", "am-msg bad", state.error) : null,
    el("div", "am-line ip-foot", go, button("Cancel", "dbtn quiet", closeImportProject)),
  ];
}

/// "Your fork login/repo will be cloned, upstream added", once read for
/// the repository typed and when no checkout was picked.
function forkLine() {
  if (previewFor !== state.repo.trim() || state.chosen?.origin != null) return null;
  const note = forkNote(fork);
  return note ? el("div", "ip-found ip-fork", el("span", "ddot"), note) : null;
}

/// The checkouts found and the branches with work, each with where its worktree
/// goes or Skip.
function branchesBlock() {
  if (!previewFor || previewFor !== state.repo.trim()) return [];
  const label = el("div", "am-label", "Branches");
  if (preview === undefined) return [label, el("div", "am-wait", el("span", "dspin"), "Looking for checkouts and branches…")];
  if (preview === null) return previewError ? [label, el("div", "am-sub", previewError)] : [];
  const { default_branch } = preview;
  const choices = placeChoices(preview, machines);
  const rows = preview.branches.map(b => {
    const meta = previewLine(b, default_branch);
    const line = el("div", "ip-branch", el("code", "", b.name), el("span", "m", meta));
    line.title = `${b.name}  ${meta}`;
    if (b.name !== default_branch && !b.worktree) {
      const pick = (id: string | null, label: string) => {
        const x = button(label, "", () => { if (id == null) delete picks[b.name]; else picks[b.name] = id; draw(); });
        x.setAttribute("aria-pressed", String(id == null ? !(b.name in picks) : picks[b.name] === id));
        return x;
      };
      line.append(el("span", "lbl", "Worktree"), el("span", "seg", ...choices.map(c => pick(c.id, c.name)), pick(null, "Skip")));
    }
    return line;
  });
  return [
    label,
    el("div", "am-sub", [checkoutNote(preview), preview.note].filter(Boolean).join(" ")),
    rows.length ? el("div", "ip-branches", ...rows) : el("div", "am-sub", "No branches with work."),
  ];
}

/// The parent a clone goes under before anything is typed: the last one used
/// on the picked machine, else its projects folder.
function defaultParent() {
  if (!state.machine) return null;
  const last = lastImport();
  if (last?.root && last.machine === state.machine) return last.root;
  const base = bases.get(state.machine);
  if (!base) return null;
  const os = machines.find(x => x.id === state.machine)?.env?.os;
  return `${base.replace(/\/+$/, "")}/${os === "linux" ? "projects" : "Projects"}`;
}

/// "Goes in <parent>/<org>/<repo>." under the parent field.
function goesIn() {
  const parent = parentFolder();
  if (!parent) return "";
  return `Goes in ${folderUnder(parent, state.repo) ?? `${parent.replace(/\/+$/, "")}/<org>/<repo>`}.`;
}

/// Where it goes, the default or the folder chosen or typed, and Choose….
function folderLine() {
  const path = el("input", "ip-path ip-dir");
  path.type = "text";
  path.spellcheck = false;
  path.autocapitalize = "off";
  path.setAttribute("autocorrect", "off");
  path.setAttribute("aria-label", state.chosen ? "Folder" : "Clone into");
  path.setAttribute("role", "combobox");
  path.setAttribute("aria-autocomplete", "list");
  path.setAttribute("aria-controls", "ip-menu");
  path.setAttribute("aria-expanded", "false");
  path.placeholder = state.machine ? "The machine's projects folder" : "The default folder on the machine Beep picks";
  // On Auto there is no machine to name a folder on.
  path.disabled = !state.machine;
  path.value = path.title = state.machine ? folderText ?? state.chosen?.path ?? defaultParent() ?? "" : "";
  path.addEventListener("input", () => {
    // Typing over a chosen folder makes it a parent again.
    if (state.chosen) { const text = path.value; update({ type: "unchoose" }); folderText = text; draw(); return; }
    folderText = path.title = path.value;
    syncForm();
    lookFolders();
  });
  path.addEventListener("keydown", e => onFolderKey(e, path));
  path.addEventListener("blur", () => { lookup++; clearTimeout(lookupTimer); setMenu(NO_MENU); });
  const pick = button(reading ? "Reading…" : "Choose…", "dbtn quiet", choose);
  pick.disabled = !machines.length || reading;
  const reset = state.chosen ? button("Default", "dbtn quiet", () => update({ type: "unchoose" })) : null;
  return el("div", "am-line", el("div", "ip-folder", path), pick, reset);
}

/// Forgets the folder typed and its menu.
function resetFolder() {
  folderText = null;
  menu = NO_MENU;
  lookup++;
  clearTimeout(lookupTimer);
}

/// Up, Down, Tab and Enter in the Folder field (Esc reaches `onKey`
/// first). Enter with no menu imports.
function onFolderKey(e: KeyboardEvent, field: HTMLInputElement) {
  const act = menuKey(menu, e.key, e.shiftKey || e.altKey || e.ctrlKey || e.metaKey);
  if (act.type === "none") {
    if (e.key === "Enter") { e.preventDefault(); submit(); }
    return;
  }
  e.preventDefault();
  if (act.type === "move") setMenu({ ...menu, index: act.index });
  else if (act.type === "close") setMenu(NO_MENU);
  else acceptFolder(field, act.text);
}

/// Puts a folder picked from the menu in the field, then offers its folders.
function acceptFolder(field: HTMLInputElement, text: string) {
  folderText = field.value = field.title = text;
  field.setSelectionRange(text.length, text.length);
  lookFolders();
}

/// Offers the folders matching what the Folder field holds, once typing pauses;
/// a folder read before answers at once.
function lookFolders() {
  clearTimeout(lookupTimer);
  const ask = ++lookup;
  const machine = state.machine;
  const typed = folderText == null ? null : splitTyped(folderText);
  if (!machine || !typed) { setMenu(NO_MENU); return; }
  if (menu.dir !== typed.dir) setMenu(NO_MENU);
  const key = `${machine}\n${typed.dir}`;
  const offer = (folders: readonly string[]) => {
    if (ask === lookup && state.machine === machine) setMenu(menuOf(typed.dir, suggest(folders, typed.prefix)));
  };
  const known = listed.get(key);
  if (known) { offer(known); return; }
  lookupTimer = setTimeout(async () => {
    try {
      const listing = await invoke<MachineFolder>("list_machine_folder", { machineId: machine, path: typed.dir });
      listed.set(key, listing.folders);
      offer(listing.folders);
    } catch {
      // Nothing to offer: typing goes on as without the menu.
      if (ask === lookup) setMenu(NO_MENU);
    }
  }, 150);
}

/// Shows `next` under the Folder field, in place, while the field has focus.
function setMenu(next: FolderMenu) {
  const field = sheet?.querySelector<HTMLInputElement>(".ip-dir");
  menu = field && document.activeElement === field ? next : NO_MENU;
  if (!field) return;
  const box = field.parentElement!;
  box.querySelector(".ip-menu")?.remove();
  field.setAttribute("aria-expanded", String(isOpen(menu)));
  field.removeAttribute("aria-activedescendant");
  if (!isOpen(menu)) return;
  const { dir, index } = menu;
  const list = el("div", "ip-menu", ...menu.items.map((name, i) => {
    const row = el("div", i === index ? "on" : "", name);
    row.id = `ip-menu-${i}`;
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", String(i === index));
    // The field keeps focus, so typing goes on after a click.
    row.addEventListener("mousedown", e => e.preventDefault());
    row.addEventListener("click", () => acceptFolder(field, completed(dir, name)));
    return row;
  }));
  list.id = "ip-menu";
  list.setAttribute("role", "listbox");
  field.setAttribute("aria-activedescendant", `ip-menu-${index}`);
  box.append(list);
  list.children[index]?.scrollIntoView({ block: "nearest" });
}

/// The folder browser for another machine: the folder, its folders, and
/// what importing there does.
function browser(listing: MachineFolder) {
  const up = button("Up", "dbtn quiet ip-up", () => browseTo(listing.parent));
  up.disabled = !listing.parent || reading;
  const where = el("span", "", listing.path);
  where.title = listing.path;
  const into = (name: string) => `${listing.path.replace(/\/+$/, "")}/${name}`;
  const dirs = el("div", "dirs", ...listing.folders.map(name => {
    const b = button(name, "", () => browseTo(into(name)));
    b.disabled = reading;
    return b;
  }));
  if (!listing.folders.length) dirs.append(el("span", "none", listing.exists ? "No folders in here" : "Not there yet"));
  if (listing.more) dirs.append(el("span", "none", `and ${listing.more} more`));
  const foot = el("div", "foot", el("span", "note", folderNote(listing, state.repo)),
    button("Cancel", "dbtn quiet", () => update({ type: "browse", listing: null })), button("Use this folder", "dbtn pri", () => update({ type: "choose", listing })));
  return el("div", "ip-browse", el("div", "at", up, where), dirs, foot);
}

/// Loads the field's styles once; the Add a machine sheet's come first.
function injectStyle() {
  if (document.querySelector("style[data-ip]")) return;
  const style = document.createElement("style");
  style.dataset.ip = "";
  style.textContent = ".ip-field { flex: 1; min-width: 0; padding: 8px 11px; border-radius: 8px; border: 1px solid var(--border); background: var(--deep); color: var(--fg); font: inherit; font-size: 13px; outline: 0; caret-color: var(--accent) } .ip-field:focus { border-color: var(--accent) } .ip-sheet .am-sub { margin-top: 0 }"
    + " .ip-path { flex: 1; min-width: 0; padding: 7px 11px; border-radius: 8px; border: 1px solid var(--border); background: var(--deep); font-family: var(--mono); font-size: 12.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }"
    + " .ip-dir { width: 100%; box-sizing: border-box; color: var(--fg); outline: 0; caret-color: var(--accent) } .ip-dir:focus { border-color: var(--accent) }"
    + " .ip-dir::placeholder { color: var(--muted); font-family: system-ui, sans-serif } .ip-dir:disabled { opacity: 1 }"
    + " .ip-folder { position: relative; flex: 1; min-width: 0; display: flex }"
    + " .ip-menu { position: absolute; top: calc(100% + 4px); left: 0; right: 0; z-index: 3; max-height: 264px; overflow: auto; padding: 4px 0; border: 1px solid var(--border); border-radius: 8px; background: var(--deep); box-shadow: 0 10px 30px rgba(0,0,0,.45) }"
    + " .ip-menu div { padding: 4px 11px; font-family: var(--mono); font-size: 12.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; cursor: pointer }"
    + " .ip-menu div:hover { background: rgba(255,255,255,.05) } .ip-menu div.on { background: rgba(52, 211, 153, .14) }"
    + " .ip-browse { border: 1px solid var(--border); border-radius: 10px; background: var(--deep); overflow: hidden }"
    + " .ip-browse .at { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-bottom: 1px solid var(--border); font-family: var(--mono); font-size: 12.5px }"
    + " .ip-browse .at span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }"
    + " .ip-browse .dirs { max-height: 168px; overflow: auto; padding: 4px 0; display: flex; flex-direction: column }"
    + " .ip-browse .dirs button { all: unset; box-sizing: border-box; width: 100%; padding: 5px 12px; font-size: 13px; cursor: pointer }"
    + " .ip-browse .dirs button:hover, .ip-browse .dirs button:focus-visible { background: rgba(255,255,255,.05) }"
    + " .ip-browse .dirs .none { padding: 5px 12px; font-size: 12.5px; color: var(--muted) }"
    + " .ip-browse .foot { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-top: 1px solid var(--border); font-size: 12px; color: var(--muted) }"
    + " .ip-browse .foot .note { flex: 1; min-width: 0 }"
    + " .ip-found { display: flex; gap: 8px; align-items: center; font-size: 12.5px } .ip-fork { font-size: 12px; color: var(--muted) }"
    + " .ip-sheet { max-height: calc(100vh - 48px); overflow: auto }"
    + " .ip-foot { position: sticky; bottom: -18px; margin: 12px -20px -18px; padding: 12px 20px 18px; background: var(--surface); border-top: 1px solid var(--border) }"
    + " .ip-branches { display: flex; flex-direction: column; gap: 2px }"
    + " .ip-branch { display: flex; flex-wrap: wrap; gap: 4px 8px; align-items: center; padding: 5px 0; font-size: 12.5px; border-bottom: 1px solid var(--border) }"
    + " .ip-branch:last-child { border-bottom: 0 }"
    + " .ip-branch code { font-family: var(--mono) } .ip-branch .m { flex: 1; min-width: 0; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap }"
    + " .ip-branch .lbl { margin-left: auto; color: var(--muted); font-size: 11.5px }"
    + " .ip-branch .seg { display: inline-flex; border: 1px solid var(--border); border-radius: 7px; overflow: hidden }"
    + " .ip-branch .seg button { all: unset; cursor: pointer; padding: 3px 9px; font-size: 12px } .ip-branch .seg button + button { border-left: 1px solid var(--border) }"
    + " .ip-branch .seg button[aria-pressed=true] { background: color-mix(in srgb, var(--accent) 22%, transparent); color: var(--fg) }";
  document.head.append(style);
}
