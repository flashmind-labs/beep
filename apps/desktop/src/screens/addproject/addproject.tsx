import { createRef, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { AddProjectView } from "./addproject-view.tsx";
import { syncthingImportChoice } from "../project/syncthing.ts";
// addproject/: the Add a project, Add a folder, Import a project and New
// project sheets. Entry: openAddProject here, openAddFolder in addfolder.ts,
// openImportProject in importproject.ts, openNewProject in newproject.ts.
// Setup actions and discovered checkouts, followed by repositories on connected
// hosts. Each online machine refreshes its cached discovery list on opening.
import { invoke } from "../../ui/tauri.ts";
import { openSheet } from "../../ui/parts.tsx";
import { IMPORTS_AT_ONCE, eachLimited, foundMatches, importFound, rescanAll, tickable, toImport, toggleAllShown, unreadText } from "../../stores/addproject/addproject-state.ts";
import type { ImportState } from "../../stores/addproject/addproject-state.ts";
import type { FoundRow, ProjectRow } from "../../stores/project/projects-state.ts";
import { reason } from "../../ui/format.ts";

/// What the window hands the sheet.
export interface AddProjectOptions {
  /// The machines Home lists.
  machines?: readonly { online: boolean }[];
  /// Runs once a found folder is in; after a batch, with the first that landed.
  onImported?: (project: ProjectRow) => void;
  /// Opens the Add a folder sheet.
  onChoose?: () => void;
  /// Opens the Import a project sheet, with `repo` filled in when given.
  onClone?: (repo?: string) => void;
  /// Opens the New project sheet.
  onNew?: () => void;
}

/// One git host's repositories to clone.
export interface HostRepos {
  host: string;
  github: boolean;
  repos: { name: string; url: string; updated_at: number }[];
  error: string | null;
}

let syncthing = syncthingImportChoice();
/// The search field and the box of found rows, while the sheet shows them.
const searchField = createRef<HTMLInputElement>(), rowBox = createRef<HTMLDivElement>();
let root: HTMLDivElement | null = null, opts: AddProjectOptions = {}, close: (() => void) | null = null;
/// null while the machines look, then the rows they found.
let found: FoundRow[] | null = null;
/// null until Beep lists the repositories to clone.
let hosts: HostRepos[] | null = null;
let error: string | null = null;
/// The machines still looking again, and those that could not say, by id.
const looking = new Map<string, string>();
const unread = new Map<string, string>();
/// Counts the opens, so an answer for a sheet since closed is dropped.
let opened = 0;
let view: ((content: ReactNode) => void) | null = null;
/// The row being imported by its own button, by name, and why one failed.
let importing: string | null = null;
const failed = new Map<string, string>();
/// What the search holds, the ticked rows by name, and each batch import.
let query = "";
let picked = new Set<string>();
const status = new Map<string, ImportState>();
/// True while a batch imports.
let batch = false;


/// Opens the sheet.
export function openAddProject(options: AddProjectOptions = {}) {
  if (root) return;
  syncthing = syncthingImportChoice();
  void syncthing.anywhere();
  opts = options;
  found = null;
  hosts = null;
  error = null;
  importing = null;
  failed.clear();
  query = "";
  picked = new Set();
  status.clear();
  batch = false;
  ({ root, close, render: view } = openSheet("Add a project", "am-sheet ap-sheet", { canClose: () => !busy(), onKey, onClose: closed }));
  draw();
  looking.clear();
  unread.clear();
  const open = ++opened;
  const live = () => open === opened && !!root;
  invoke<FoundRow[]>("found_projects")
    .then(rows => { if (live() && found === null) found = rows; })
    .catch(e => { if (live() && found === null) { found = []; error = reason(e); } })
    .finally(() => { if (live()) draw(); });
  rescanAll(invoke, machines => {
    if (!live()) return;
    for (const m of machines) looking.set(m.id, m.name);
    draw();
  }, (machine, after) => {
    if (!live()) return;
    looking.delete(machine.id);
    if (after.rows) found = after.rows;
    if (after.problem) unread.set(machine.id, unreadText(machine.name, after.problem));
    else unread.delete(machine.id);
    draw();
  });
  invoke<HostRepos[]>("host_repos")
    .then(got => { if (live()) hosts = got; }, e => { if (live()) { console.warn("host repos:", e); hosts = []; } })
    .finally(() => { if (live()) draw(); });
}

function closeAddProject() {
  close?.();
}

function closed() {
  syncthing.dispose();
  view = null;
  root = close = null;
}

const busy = () => !!importing || batch;

/// The rows the search shows.
function shownRows() {
  return (found || []).filter(row => foundMatches(row, query));
}

/// The rows' elements, in list order.
function rowEls() {
  return [...rowBox.current?.querySelectorAll<HTMLElement>(".ap-row") ?? []];
}

function onKey(e: KeyboardEvent) {
  const stop = () => { e.preventDefault(); e.stopPropagation(); };
  const search = searchField.current;
  if (e.key === "Escape" || e.isComposing || !search) return;
  if (e.metaKey && e.key === "Enter") { stop(); importPicked(); return; }
  const at = document.activeElement;
  const row = at instanceof HTMLElement && at.classList.contains("ap-row") ? at : null;
  if (e.key === " " && row) { stop(); toggle(row.dataset.name || ""); return; }

  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    const rows = rowEls();
    if (!rows.length || (!row && at !== search)) return;
    stop();
    const i = row ? rows.indexOf(row) : -1;
    const to = e.key === "ArrowDown" ? Math.min(i + 1, rows.length - 1) : i - 1;
    (to < 0 ? search : rows[to]).focus();
    return;
  }

  // Typing anywhere else in the sheet goes to the search.
  if (e.key.length === 1 && e.key !== " " && !e.metaKey && !e.ctrlKey && !e.altKey && at !== search) search.focus();
}

function toggle(name: string) {
  if (!name || batch || !tickable(status.get(name))) return;
  if (picked.has(name)) picked.delete(name);
  else picked.add(name);
  draw();
}

/// Imports the ticked rows the search shows, a few at a time.
async function importPicked() {
  if (busy() || !found) return;
  const byName = new Map(found.map(row => [row.name, row]));
  const names = toImport(shownRows().map(row => row.name), picked, status);
  if (!names.length) return;
  batch = true;
  const mine = opened, then = opts.onImported;
  const live = () => mine === opened && !!root;
  const syncthingFiles = syncthing.value();

  for (const name of names) {
    status.set(name, { state: "importing" });
    failed.delete(name);
  }

  draw();
  const landed: ProjectRow[] = [];
  await eachLimited(names, IMPORTS_AT_ONCE, async name => {
    const row = byName.get(name);
    if (!row) return;

    try {
      landed.push(await importFound(invoke, row, syncthingFiles));
      if (live()) { status.set(name, { state: "done" }); picked.delete(name); }
    } catch (e) {
      if (live()) status.set(name, { state: "failed", why: reason(e) });
    }

    if (live()) draw();
  });
  if (live()) {
    batch = false;
    if (names.every(name => status.get(name)?.state === "done")) closeAddProject();
    else draw();
  }
  if (landed[0]) then?.(landed[0]);
}

async function doImport(row: FoundRow) {
  if (busy()) return;
  const mine = opened, then = opts.onImported;
  importing = row.name;
  failed.delete(row.name);
  status.delete(row.name);
  draw();

  try {
    const project = await importFound(invoke, row, syncthing.value());
    if (mine === opened && root) { importing = null; closeAddProject(); }
    then?.(project);
  } catch (e) {
    if (mine === opened && root) { importing = null; failed.set(row.name, reason(e)); draw(); }
  }
}

function draw() {
  if (!view) return;
  const current = view;
  const active = document.activeElement instanceof HTMLElement ? document.activeElement.closest<HTMLElement>(".ap-row") : null;
  flushSync(() => current(<AddProjectView
    searchField={searchField} rowBox={rowBox} found={found} shown={shownRows()} hosts={hosts} machines={opts.machines} query={query} looking={[...looking.values()]} unread={[...unread.values()]} error={error} importing={importing} batch={batch} picked={picked} status={status} failed={failed} sync={syncthing}
    search={(value: string) => { query = value; draw(); }} toggle={toggle} toggleAll={() => { picked = toggleAllShown(shownRows().map(row => row.name), picked, status); draw(); }}
    importPicked={() => void importPicked()} importOne={(row: FoundRow) => void doImport(row)}
    choose={() => { const action = opts.onChoose; closeAddProject(); action?.(); }}
    clone={(repo?: string) => { const action = opts.onClone; closeAddProject(); action?.(repo); }}
    create={() => { const action = opts.onNew; closeAddProject(); action?.(); }}
  />));
  if (active && !active.isConnected) {
    const search = searchField.current;
    if (search && !search.hidden) search.focus();
  }
}

