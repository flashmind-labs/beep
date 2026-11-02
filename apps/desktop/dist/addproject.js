// The "Add a project" sheet: the git checkouts your coding agents worked in
// (Claude Code, Codex, pi and flsh sessions each machine finds), each with
// Import, then "Choose a folder…" and "Clone a repository…", which open the
// Import a project sheet. Import links the folder where it is, on every
// machine that has it, so its sessions come along. Esc closes it.
import { agentLabel, foundSessions, importPlan, lookingText, present } from "./projects-state.js";

const { invoke } = window.__TAURI__.core;

let root = null, sheet = null, opts = {}, back = null;
/// null while the machines look, then the rows they found.
let found = null;
let error = null;
/// The row being imported, by name, and why one failed.
let importing = null;
const failed = new Map();

function el(tag, cls, ...kids) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  for (const k of kids) if (k != null && k !== false) e.append(k);
  return e;
}

function button(label, cls, onClick) {
  const b = el("button", cls, label);
  b.type = "button";
  b.addEventListener("click", onClick);
  return b;
}

/// Imports a found row in place on each machine that has it. Resolves to the
/// first import's project; a failure on one machine after another worked is
/// logged, not thrown, so the project still lands.
export async function importFound(row) {
  let first = null;
  for (const one of importPlan(row)) {
    try {
      const project = await invoke("import_project", one);
      first = first || project;
    } catch (e) {
      if (!first) throw e;
      console.warn(`linking ${row.name} on another machine:`, e);
    }
  }
  if (!first) throw new Error("Nothing to import.");
  return first;
}

/// Opens the sheet. `options.machines` are the machines Home lists;
/// `options.onImported(project)` runs once a found folder is in,
/// `options.onChoose()` and `options.onClone()` open the Import a project
/// sheet for a folder or a repository.
export function openAddProject(options = {}) {
  if (root) return;
  opts = options;
  back = document.activeElement;
  found = null;
  error = null;
  importing = null;
  failed.clear();
  injectStyle();
  root = el("div", "am-scrim");
  root.addEventListener("mousedown", e => { if (e.target === root && !importing) closeAddProject(); });
  sheet = el("div", "am-sheet ap-sheet");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", "Add a project");
  root.append(sheet);
  document.body.append(root);
  document.addEventListener("keydown", onKey, true);
  draw();
  invoke("found_projects")
    .then(rows => { found = rows; })
    .catch(e => { found = []; error = String(e).replace(/^(bad request|error):\s*/i, ""); })
    .finally(() => { if (root) draw(); });
}

export function closeAddProject() {
  if (!root) return;
  document.removeEventListener("keydown", onKey, true);
  root.remove();
  root = sheet = null;
  if (back && back.focus) back.focus();
}

function onKey(e) {
  if (e.key !== "Escape") return;
  e.preventDefault();
  e.stopPropagation();
  if (!importing) closeAddProject();
}

async function doImport(row) {
  if (importing) return;
  importing = row.name;
  failed.delete(row.name);
  draw();
  try {
    const project = await importFound(row);
    importing = null;
    closeAddProject();
    opts.onImported?.(project);
  } catch (e) {
    importing = null;
    failed.set(row.name, String(e).replace(/^(bad request|error):\s*/i, ""));
    if (root) draw();
  }
}

function draw() {
  const head = el("div", "am-head", el("b", "", "Add a project"), el("kbd", "", "esc"));
  const list = found === null
    ? el("div", "am-wait", el("span", "dspin"), lookingText(opts.machines))
    : found.length
      ? el("div", "ap-list", ...found.map(row => foundRow(row, { busy: importing, error: failed.get(row.name) })))
      : el("div", "am-sub", error || "No other folders yet. Folders show up here once a coding agent has worked in them.");
  const ways = el("div", "ap-ways",
    way("Choose a folder…", "Anywhere on this Mac, or browse another machine", () => { closeAddProject(); opts.onChoose?.(); }),
    way("Clone a repository…", "org/repo or a URL from GitHub or your Forgejo server", () => { closeAddProject(); opts.onClone?.(); }));
  sheet.replaceChildren(...present([
    head,
    el("div", "am-label", "Found on your machines"),
    list,
    el("div", "am-sub", "Import links the folder where it is and brings its sessions along. Your other machines with the same repository are linked too."),
    ways,
  ]));
}

function way(title, sub, onClick) {
  return button(el("span", "", el("b", "", title), el("span", "", sub)), "", onClick);
}

/// One found checkout: its name, where it is on each machine, its sessions
/// per agent and when one last ran, and Import. `state.busy` names the row
/// being imported; `state.error` is why this one failed. `onImport`
/// defaults to importing it from this sheet.
export function foundRow(row, state = {}, onImport = doImport) {
  const go = button(state.busy === row.name ? "Importing…" : "Import", `dbtn${state.quiet ? "" : " pri"} small`, () => onImport(row));
  go.disabled = !!state.busy;
  go.title = `Import ${row.name} with ${foundSessions(row)} sessions`;
  const places = row.places.map(p => {
    const where = el("span", "", el("b", "", p.machine_name), " ", el("code", "", p.folder));
    where.title = `${p.machine_name}: ${p.folder}`;
    return el("span", "m", where);
  });
  const agents = el("span", "m", ...row.agents.map(a => el("span", "ap-ag", `${agentLabel(a.agent)} ${a.sessions}`)),
    row.last_activity ? el("span", "", ago(row.last_activity)) : null);
  return el("div", "ap-row",
    el("span", "n", row.name),
    ...places,
    agents,
    state.error ? el("span", "m bad", state.error) : null,
    el("span", "acts", go));
}

/// "12 min ago", "yesterday", "3 days ago".
export function ago(ms, now = Date.now()) {
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
  if (!document.querySelector("link[data-am]")) {
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.href = new URL("./addmachine.css", import.meta.url).href;
    link.dataset.am = "";
    document.head.append(link);
  }
  if (document.querySelector("style[data-ap]")) return;
  const style = document.createElement("style");
  style.dataset.ap = "";
  style.textContent = ".ap-sheet { width: min(640px, 88vw) } .ap-sheet .am-sub { margin-top: 0 }"
    + " .ap-list { border: 1px solid var(--border); border-radius: 10px; background: var(--elev, #0b100e); padding: 2px 12px; max-height: 300px; overflow: auto }"
    + " .ap-row { display: grid; grid-template-columns: minmax(0,1fr) auto; gap: 4px 12px; align-items: center; padding: 9px 0; border-top: 1px solid var(--border) }"
    + " .ap-row:first-child { border-top: 0 }"
    + " .ap-row .n { font-size: 13.5px; font-weight: 600; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }"
    + " .ap-row .m { grid-column: 1; display: flex; flex-wrap: wrap; gap: 4px 10px; font-size: 12px; color: var(--muted); min-width: 0 }"
    + " .ap-row .m > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }"
    + " .ap-row .m b { font-weight: 500; color: var(--fg) }"
    + " .ap-row .m code { font-size: 11.5px }"
    + " .ap-row .m.bad { color: var(--err, #e5826f); white-space: normal }"
    + " .ap-row .acts { grid-row: 1 / span 4; grid-column: 2; display: flex; gap: 6px }"
    + " .ap-ag { display: inline-flex; gap: 4px; align-items: center; padding: 0 6px; border-radius: 6px; border: 1px solid var(--border); font-size: 11px; line-height: 17px; color: var(--fg) }"
    + " .ap-ways { display: grid; grid-template-columns: 1fr 1fr; gap: 8px }"
    + " .ap-ways button { display: grid; gap: 2px; text-align: left; padding: 10px 12px; border-radius: 10px; border: 1px solid var(--border); background: rgba(255,255,255,.03); color: var(--fg); font: inherit; cursor: pointer }"
    + " .ap-ways button > span { display: grid; gap: 2px }"
    + " .ap-ways button span span { font-size: 12px; color: var(--muted) }";
  document.head.append(style);
}
