// The "Add a project" sheet: the git checkouts your coding agents worked in
// (Claude Code, Codex, pi, flsh and OpenCode sessions each machine finds),
// each with Import, then the user's repositories on GitHub and Forgejo that no
// machine has, each with Clone, then "Choose a folder…" and "Clone a
// repository…", which open the Import a project sheet.
import { invoke } from "../ui/tauri.ts";
import { focusBack, linkSheetStyle, sheetShell } from "../machines/addmachine.ts";
import { IMPORTS_AT_ONCE, eachLimited, foundMatches, tickable, toImport, toggleAllShown } from "./addproject-state.ts";
import type { ImportState } from "./addproject-state.ts";
import { agentLabel, foundSessions, importPlan, lookingText } from "../project/projects-state.ts";
import { button, el } from "../ui/dom.ts";
import { reason } from "./importproject-state.ts";
import type { FoundRow, ProjectRow } from "../project/projects-state.ts";

/// What the window hands the sheet.
export interface AddProjectOptions {
  /// The machines Home lists.
  machines?: readonly { online: boolean }[];
  /// Runs once a found folder is in; after a batch, with the first that landed.
  onImported?: (project: ProjectRow) => void;
  /// Opens the Import a project sheet for a folder.
  onChoose?: () => void;
  /// Opens the Import a project sheet, with `repo` filled in when given.
  onClone?: (repo?: string) => void;
}

/// One git host's repositories to clone.
interface HostRepos {
  host: string;
  github: boolean;
  repos: { name: string; url: string; updated_at: number }[];
  error: string | null;
}

/// The most repositories one host shows.
const REPOS_SHOWN = 30;

/// How a found row shows: `busy` names the row being imported (any non-null
/// value disables Import), `error` is why this one failed, `quiet` drops the
/// primary look from Import.
export interface FoundRowState {
  busy?: string | null;
  error?: string | null;
  quiet?: boolean;
  pick?: { on: boolean; toggle: () => void } | null;
  status?: ImportState | null;
}

let root: HTMLDivElement | null = null, sheet: HTMLDivElement | null = null, opts: AddProjectOptions = {}, back: Element | null = null;
/// null while the machines look, then the rows they found.
let found: FoundRow[] | null = null;
/// null until the server lists the repositories to clone.
let hosts: HostRepos[] | null = null;
let error: string | null = null;
/// The row being imported by its own button, by name, and why one failed.
let importing: string | null = null;
const failed = new Map<string, string>();
/// What the search holds, the ticked rows by name, and each batch import.
let query = "";
let picked = new Set<string>();
const status = new Map<string, ImportState>();
/// True while a batch imports.
let batch = false;
let search: HTMLInputElement | null = null, bar: HTMLDivElement | null = null, listBox: HTMLDivElement | null = null, remoteBox: HTMLDivElement | null = null;

/// Imports a found row in place on each machine that has it.
export async function importFound(row: FoundRow) {
  let first: ProjectRow | null = null;
  for (const one of importPlan(row)) {
    try {
      const project = await invoke<ProjectRow>("import_project", one);
      first = first || project;
    } catch (e) {
      if (!first) throw e;
      console.warn(`linking ${row.name} on another machine:`, e);
    }
  }
  if (!first) throw new Error("Nothing to import.");
  return first;
}

/// Opens the sheet.
export function openAddProject(options: AddProjectOptions = {}) {
  if (root) return;
  opts = options;
  back = document.activeElement;
  found = null;
  hosts = null;
  error = null;
  importing = null;
  failed.clear();
  query = "";
  picked = new Set();
  status.clear();
  batch = false;
  injectStyle();
  ({ root, sheet } = sheetShell("Add a project", "am-sheet ap-sheet"));
  root.addEventListener("mousedown", e => { if (e.target === root && !busy()) closeAddProject(); });
  build();
  document.body.append(root);
  document.addEventListener("keydown", onKey, true);
  draw();
  invoke<FoundRow[]>("found_projects")
    .then(rows => { found = rows; })
    .catch(e => { found = []; error = reason(e); })
    .finally(() => { if (root) draw(); });
  invoke<HostRepos[]>("host_repos")
    .then(got => { hosts = got; }, e => { console.warn("host repos:", e); hosts = []; })
    .finally(() => { if (root) draw(); });
}

export function closeAddProject() {
  if (!root) return;
  document.removeEventListener("keydown", onKey, true);
  root.remove();
  root = sheet = search = bar = listBox = remoteBox = null;
  focusBack(back);
}

const busy = () => !!importing || batch;

/// The rows the search shows.
function shownRows() {
  return (found || []).filter(row => foundMatches(row, query));
}

/// The rows' elements, in list order.
function rowEls() {
  return [...(listBox?.querySelectorAll<HTMLElement>(".ap-row") || [])];
}

function onKey(e: KeyboardEvent) {
  const stop = () => { e.preventDefault(); e.stopPropagation(); };
  if (e.key === "Escape") {
    stop();
    if (!busy()) closeAddProject();
    return;
  }
  if (e.isComposing || !search) return;
  if (e.metaKey && e.key === "Enter") { stop(); importPicked(); return; }
  const at = document.activeElement;
  const row = at instanceof HTMLElement && at.classList.contains("ap-row") ? at : null;
  if (e.key === " " && row) {
    stop();
    toggle(row.dataset.name || "");
    return;
  }
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
      landed.push(await importFound(row));
      status.set(name, { state: "done" });
      picked.delete(name);
    } catch (e) {
      status.set(name, { state: "failed", why: reason(e) });
    }
    if (root) draw();
  });
  batch = false;
  if (names.every(name => status.get(name)?.state === "done")) closeAddProject();
  else if (root) draw();
  if (landed[0]) opts.onImported?.(landed[0]);
}

async function doImport(row: FoundRow) {
  if (busy()) return;
  importing = row.name;
  failed.delete(row.name);
  status.delete(row.name);
  draw();
  try {
    const project = await importFound(row);
    importing = null;
    closeAddProject();
    opts.onImported?.(project);
  } catch (e) {
    importing = null;
    failed.set(row.name, reason(e));
    if (root) draw();
  }
}

/// The sheet's fixed parts: the search, the bar over the list and the list,
/// which `draw` fills, so the search keeps its text and caret.
function build() {
  search = el("input", "ap-q");
  search.type = "search";
  search.placeholder = "Filter by repo, folder, machine or agent";
  search.autocomplete = "off";
  search.spellcheck = false;
  search.setAttribute("aria-label", "Filter found folders");
  search.addEventListener("input", () => { query = search!.value; draw(); });
  bar = el("div", "ap-bar");
  listBox = el("div", "ap-box");
  remoteBox = el("div", "ap-remote");
  const head = el("div", "am-head", el("b", "", "Add a project"), el("kbd", "", "esc"));
  const ways = el("div", "ap-ways",
    way("Choose a folder…", "Anywhere on this Mac, or browse another machine", () => { closeAddProject(); opts.onChoose?.(); }),
    way("Clone a repository…", "org/repo or a URL from GitHub or your Forgejo server", () => { closeAddProject(); opts.onClone?.(); }));
  sheet!.replaceChildren(
    head,
    el("div", "am-label", "Found on your machines"),
    search,
    bar,
    listBox,
    remoteBox,
    ways);
}

function draw() {
  if (!search || !bar || !listBox) return;
  const any = !!found?.length;
  const remote = drawRemote();
  bar.hidden = !any;
  search.hidden = !any && !remote;
  const focused = document.activeElement instanceof HTMLElement ? document.activeElement.closest<HTMLElement>(".ap-row") : null;
  const keep = focused && listBox.contains(focused) ? focused.dataset.name : null;
  const shown = shownRows();
  // The list is drawn anew, so it takes the old one's scroll with it.
  const scrolled = listBox.querySelector<HTMLElement>(".ap-list")?.scrollTop ?? 0;
  if (found === null) {
    listBox.replaceChildren(el("div", "am-wait", el("span", "dspin"), lookingText(opts.machines)));
  } else if (!found.length) {
    listBox.replaceChildren(el("div", "am-sub", error || "No other folders yet."));
  } else if (!shown.length) {
    listBox.replaceChildren(el("div", "am-sub", `Nothing found matches “${query.trim()}”.`));
  } else {
    listBox.replaceChildren(el("div", "ap-list", ...shown.map(row => foundRow(row, {
      busy: importing || (batch ? "" : null),
      error: failed.get(row.name),
      pick: { on: picked.has(row.name), toggle: () => toggle(row.name) },
      status: status.get(row.name),
    }))));
    listBox.querySelector<HTMLElement>(".ap-list")!.scrollTop = scrolled;
  }
  if (keep) rowEls().find(r => r.dataset.name === keep)?.focus({ preventScroll: true });
  else if (!search.hidden && !sheet?.contains(document.activeElement)) search.focus();
  if (!any) return;
  const names = shown.map(row => row.name);
  const open = names.filter(name => tickable(status.get(name)));
  const ready = toImport(names, picked, status).length;
  const all = el("input");
  all.type = "checkbox";
  all.checked = open.length > 0 && open.every(name => picked.has(name));
  all.disabled = batch || !open.length;
  all.addEventListener("change", () => { picked = toggleAllShown(names, picked, status); draw(); });
  const go = button(batch ? "Importing…" : `Import ${ready}`, "dbtn pri small", () => importPicked());
  go.append(el("kbd", "", "⌘↵"));
  go.disabled = busy() || !ready;
  const count = query.trim() ? `${shown.length} of ${found!.length} shown` : `${found!.length} found`;
  bar.replaceChildren(el("label", "", all, "Select all shown"), el("span", "", count), go);
}

/// Fills the repositories to clone, per host, as the search shows them,
/// leaving out those found on a machine; true when any host has one.
function drawRemote() {
  if (!remoteBox) return false;
  const have = new Set((found || []).map(row => row.name.toLowerCase()));
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const sections = (hosts || []).flatMap(h => {
    const repos = h.repos.filter(r => !have.has(r.name.toLowerCase()));
    const shown = repos.filter(r => words.every(w => r.name.toLowerCase().includes(w))).slice(0, REPOS_SHOWN);
    if (!h.error && !shown.length) return [];
    return [
      el("div", "am-label", `On ${h.github ? "GitHub" : h.host}`),
      h.error ? el("div", "am-sub", h.error) : el("div", "ap-list", ...shown.map(r => el("div", "ap-rm",
        el("span", "n", r.name),
        el("span", "m", ago(r.updated_at)),
        button("Clone", "dbtn small", () => { closeAddProject(); opts.onClone?.(r.url); })))),
    ];
  });
  remoteBox.replaceChildren(...sections);
  return (hosts || []).some(h => h.repos.length > 0);
}

function way(title: string, sub: string, onClick: () => void) {
  return button(el("span", "", el("b", "", title), el("span", "", sub)), "", onClick);
}

/// One found checkout: its name, where it is on each machine, its sessions per
/// agent and when one last ran, and Import.
export function foundRow(row: FoundRow, state: FoundRowState = {}, onImport: (row: FoundRow) => void = doImport) {
  const st = state.status;
  const go = button(state.busy === row.name ? "Importing…" : st?.state === "failed" ? "Retry" : "Import", `dbtn${state.quiet ? "" : " pri"} small`, () => onImport(row));
  go.disabled = state.busy != null;
  go.title = `Import ${row.name} with ${foundSessions(row)} sessions`;
  const act = st?.state === "importing" ? el("span", "ap-st", el("span", "dspin"), "Importing…")
    : st?.state === "done" ? el("span", "ap-st ok", "Imported")
    : go;
  const why = st?.state === "failed" ? `Failed: ${st.why}` : state.error;
  const places = row.places.map(p => {
    const where = el("span", "", el("b", "", p.machine_name), " ", el("code", "", p.folder));
    where.title = `${p.machine_name}: ${p.folder}`;
    return el("span", "m", where);
  });
  const agents = el("span", "m", ...row.agents.map(a => el("span", "ap-ag", `${agentLabel(a.agent)} ${a.sessions}`)),
    row.last_activity ? el("span", "", ago(row.last_activity)) : null);
  const out = el("div", `ap-row${state.pick ? " ap-pick" : ""}`,
    el("span", "n", row.name),
    ...places,
    agents,
    why ? el("span", "m bad", why) : null,
    el("span", "acts", act));
  if (state.pick) {
    const pick = state.pick;
    const box = el("input");
    box.type = "checkbox";
    box.checked = pick.on;
    box.disabled = !tickable(st ?? undefined);
    box.tabIndex = -1;
    box.setAttribute("aria-label", `Select ${row.name}`);
    box.addEventListener("change", () => pick.toggle());
    out.prepend(box);
    out.tabIndex = 0;
    out.dataset.name = row.name;
    out.setAttribute("aria-selected", String(pick.on));
  }
  return out;
}

/// "12 min ago", "yesterday", "3 days ago".
export function ago(ms: number, now = Date.now()) {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  const d = Math.round(s / 86400);
  if (d === 1) return "yesterday";
  if (d < 7) return `${d} days ago`;
  return d < 14 ? "last week" : `${Math.round(d / 7)} weeks ago`;
}

/// Loads the sheet's styles once: the Add a machine sheet's, plus the rows.
export function injectStyle() {
  linkSheetStyle();
  if (document.querySelector("style[data-ap]")) return;
  const style = document.createElement("style");
  style.dataset.ap = "";
  style.textContent = ".ap-sheet { width: min(640px, 88vw) } .ap-sheet .am-sub { margin-top: 0 }"
    + " .ap-list { border: 1px solid var(--border); border-radius: 10px; background: var(--elev); padding: 2px 12px; max-height: 300px; overflow: auto }"
    + " .ap-row { display: grid; grid-template-columns: minmax(0,1fr) auto; gap: 4px 12px; align-items: center; padding: 9px 0; border-top: 1px solid var(--border) }"
    + " .ap-row:first-child { border-top: 0 }"
    + " .ap-row .n { font-size: 13.5px; font-weight: 600; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }"
    + " .ap-row .m { grid-column: 1; display: flex; flex-wrap: wrap; gap: 4px 10px; font-size: 12px; color: var(--muted); min-width: 0 }"
    + " .ap-row .m > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }"
    + " .ap-row .m b { font-weight: 500; color: var(--fg) }"
    + " .ap-row .m code { font-size: 11.5px }"
    + " .ap-row .m.bad { color: var(--err); white-space: normal }"
    + " .ap-row .acts { grid-row: 1 / span 4; grid-column: 2; display: flex; gap: 6px }"
    + " .ap-ag { display: inline-flex; gap: 4px; align-items: center; padding: 0 6px; border-radius: 6px; border: 1px solid var(--border); font-size: 11px; line-height: 17px; color: var(--fg) }"
    + " .ap-q { width: 100%; box-sizing: border-box; padding: 6px 10px; border-radius: 8px; border: 1px solid var(--border); background: rgba(255,255,255,.03); color: var(--fg); font: inherit; font-size: 13px }"
    + " .ap-q[hidden], .ap-bar[hidden] { display: none }"
    + " .ap-bar { display: flex; flex: none; align-items: center; gap: 10px; min-height: 30px; margin-bottom: 6px; font-size: 12.5px; color: var(--muted) }"
    + " .ap-bar label { display: inline-flex; align-items: center; gap: 6px; cursor: pointer }"
    + " .ap-bar .dbtn { margin-left: auto; flex: none; display: inline-flex; align-items: center; white-space: nowrap }"
    + " .ap-bar .dbtn kbd { margin-left: 6px; padding: 0 4px; font-size: 10.5px; line-height: 15px; border: 0; background: none; opacity: .7 }"
    + " .ap-row.ap-pick { grid-template-columns: 18px minmax(0,1fr) auto }"
    + " .ap-row.ap-pick > input { grid-row: 1 / span 4; grid-column: 1; align-self: start; margin: 3px 0 0 }"
    + " .ap-row.ap-pick .n, .ap-row.ap-pick .m { grid-column: 2 }"
    + " .ap-row.ap-pick .acts { grid-column: 3; align-items: center }"
    + " .ap-row:focus { outline: none } .ap-row:focus-visible { outline: 1px solid var(--accent); outline-offset: 2px; border-radius: 6px }"
    + " .ap-st { font-size: 12px; color: var(--muted); display: inline-flex; gap: 6px; align-items: center }"
    + " .ap-st.ok { color: var(--ok) }"
    + " .ap-sheet { max-height: calc(100vh - 80px); overflow: auto }"
    + " .ap-remote .ap-list { max-height: 180px }"
    + " .ap-rm { display: flex; align-items: center; gap: 10px; padding: 7px 0; border-top: 1px solid var(--border); font-size: 13px }"
    + " .ap-rm:first-child { border-top: 0 }"
    + " .ap-rm .n { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }"
    + " .ap-rm .m { font-size: 12px; color: var(--muted) }"
    + " .ap-ways { display: grid; grid-template-columns: 1fr 1fr; gap: 8px }"
    + " .ap-ways button { display: grid; gap: 2px; text-align: left; padding: 10px 12px; border-radius: 10px; border: 1px solid var(--border); background: rgba(255,255,255,.03); color: var(--fg); font: inherit; cursor: pointer }"
    + " .ap-ways button > span { display: grid; gap: 2px }"
    + " .ap-ways button span span { font-size: 12px; color: var(--muted) }";
  document.head.append(style);
}
