// The "Import a project" sheet: one field for org/repo or a URL, a chip per
// online machine (Auto lets the server pick), the folder it goes in, then
// Import. Choose… opens the macOS folder picker on this Mac and a folder
// browser on another machine; a checkout picked there fills the field and
// is imported in place. It shows the clone's progress and closes onto the
// new project. Esc or Cancel closes it, except while cloning, when the
// clone goes on and the project shows up once it is in.
import { START, next, looksLikeRepo, machineChips, progressText, doneText, present, defaultFolder, folderNote, folderHelp } from "./projects-state.js";
import { checkoutNote, defaultPlaces, placeChoices, previewLine, worktreesToAsk } from "./branchwork.js";

const { invoke } = window.__TAURI__.core;

let root = null, sheet = null, state = START, machines = [], macId = null, back = null, onDone = null;
/// The repository's checkouts and branches with work, once read: for the
/// repo text `previewFor`, null from a server too old to say. `picks` is
/// where each branch gets a worktree, by name: a machine id, `""` for
/// where the project lands, or absent to skip.
let preview = undefined, previewFor = "", previewError = "", picks = {}, previewTimer = null;
/// The folder each machine starts in (workspace, else home), by id, once read.
const bases = new Map();
/// A folder is being read, so the folder buttons wait.
let reading = false;

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

/// Opens the sheet. `options.machines` are the machines Home lists and
/// `options.macId` this Mac's machine id, when it is one;
/// `options.onDone(project)` runs once a project is in, as when the window
/// selects it. With `options.choose` it goes straight to choosing a folder.
/// `options.repo` and `options.machine` fill in the repository and pick
/// the machine, as Clone here on a project's page does.
export function openImportProject(options = {}) {
  if (root) return;
  machines = options.machines || [];
  macId = options.macId || null;
  onDone = options.onDone || null;
  back = document.activeElement;
  injectStyle();
  // Start on this Mac when it is one of the machines: Choose… then opens the
  // macOS folder picker straight away.
  state = macId && machines.some(m => m.id === macId) ? { ...START, machine: macId } : START;
  if (options.machine && machines.some(m => m.id === options.machine)) state = { ...state, machine: options.machine };
  if (options.repo) state = { ...state, repo: options.repo };
  root = el("div", "am-scrim");
  root.addEventListener("mousedown", e => { if (e.target === root && state.step !== "cloning") closeImportProject(); });
  sheet = el("div", "am-sheet ip-sheet");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", "Import a project");
  root.append(sheet);
  document.body.append(root);
  document.addEventListener("keydown", onKey, true);
  preview = undefined;
  previewFor = "";
  previewError = "";
  picks = {};
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
    if (root) draw();
    try {
      const got = await invoke("preview_import", { repo });
      if (previewFor !== repo) return;
      preview = got;
      picks = defaultPlaces(got);
    } catch (e) {
      if (previewFor !== repo) return;
      preview = null;
      previewError = String(e).replace(/^(bad request|error):\s*/i, "");
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
  if (back && back.focus) back.focus();
  const then = onDone;
  onDone = null;
  if (done && then) then(done);
}

function onKey(e) {
  if (e.key !== "Escape") return;
  e.preventDefault();
  e.stopPropagation();
  closeImportProject();
}

function update(event) {
  state = next(state, event);
  if (root && event.type !== "edit") draw();
  else if (root) syncForm();
}

async function submit() {
  const was = state;
  update({ type: "submit" });
  if (state === was) return;
  // The branches picked go with it only when they were read for this repo.
  const worktrees = preview && previewFor === state.repo.trim() ? worktreesToAsk(preview, picks) : [];
  try {
    const project = await invoke("import_project", { repo: state.repo, machineId: state.machine, folder: state.chosen?.path ?? null, worktrees });
    update({ type: "done", project });
    // The sheet may have closed while cloning: the project still lands.
    if (!root && onDone) { const then = onDone; onDone = null; then(project); }
  } catch (e) {
    update({ type: "failed", message: String(e) });
  }
}

/// Keeps the typed text and focus, only enabling Import and clearing an error.
function syncForm() {
  const go = sheet.querySelector(".ip-go");
  if (go) go.disabled = !looksLikeRepo(state.repo);
  if (!state.error) sheet.querySelector(".am-msg.bad")?.remove();
}

function draw() {
  const head = el("div", "am-head", el("b", "", "Import a project"), el("kbd", "", "esc"));
  sheet.replaceChildren(head, ...present(body()));
  const field = sheet.querySelector("input");
  const pick = sheet.querySelector(".ip-browse .dirs button");
  if (state.browsing && pick) pick.focus();
  else if (field && !state.browsing) { field.focus(); field.setSelectionRange(field.value.length, field.value.length); }
  else sheet.querySelector(".dbtn.pri")?.focus();
}

/// Reads a folder on the picked machine (`path` null for where it starts),
/// then hands it to `then`; a failure shows as the sheet's error.
async function read(path, then) {
  const machine = state.machine;
  if (!machine || reading) return;
  reading = true;
  if (root) draw();
  try {
    const listing = await invoke("list_machine_folder", { machineId: machine, path });
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
    read(state.chosen?.path ?? null, listing => update({ type: "browse", listing }));
    return;
  }
  let path = null;
  try {
    path = await invoke("pick_folder", { start: state.chosen?.path ?? bases.get(state.machine) ?? null });
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
    case "done":
      return [
        el("div", "am-done", el("span", "ddot"), el("b", "", `${state.project.name} imported`)),
        el("div", "am-sub", doneText(state.project)),
        button("Open project", "dbtn pri", closeImportProject),
      ];
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
    el("div", "am-label", "Folder"),
    state.browsing ? browser(state.browsing) : folderLine(),
    state.chosen && !state.browsing ? el("div", "ip-found", state.chosen.origin != null && el("span", "ddot"), folderNote(state.chosen, state.repo)) : null,
    el("div", "am-sub", folderHelp(state, machines)),
    ...branchesBlock(),
    state.error ? el("div", "am-msg bad", state.error) : null,
    el("div", "am-line", go, button("Cancel", "dbtn quiet", closeImportProject)),
  ];
}

/// The checkouts found and the branches with work, each with where its
/// worktree goes or Skip. Nothing before a repository is typed, or from a
/// server too old to look.
function branchesBlock() {
  if (!previewFor || previewFor !== state.repo.trim()) return [];
  if (preview === undefined) return [el("div", "am-label", "Branches"), el("div", "am-wait", el("span", "dspin"), "Looking for checkouts and branches…")];
  if (preview === null) return previewError ? [el("div", "am-label", "Branches"), el("div", "am-sub", previewError)] : [];
  const choices = placeChoices(preview, machines);
  const rows = preview.branches.map(b => {
    const line = el("div", "ip-branch", el("code", "", b.name), el("span", "m", previewLine(b, preview.default_branch)));
    if (b.name !== preview.default_branch && !b.worktree) {
      const seg = el("span", "seg");
      const pick = (id, label) => {
        const x = button(label, "", () => { if (id == null) delete picks[b.name]; else picks[b.name] = id; draw(); });
        x.setAttribute("aria-pressed", String(id == null ? !(b.name in picks) : picks[b.name] === id));
        return x;
      };
      seg.append(...choices.map(c => pick(c.id, c.name)), pick(null, "Skip"));
      line.append(el("span", "lbl", "Worktree"), seg);
    }
    return line;
  });
  return [
    el("div", "am-label", "Branches"),
    el("div", "am-sub", [checkoutNote(preview), preview.note].filter(Boolean).join(" ")),
    rows.length ? el("div", "ip-branches", ...rows) : el("div", "am-sub", "No branches with work."),
  ];
}

/// Where it goes, the default or the folder chosen, and Choose….
function folderLine() {
  const m = machines.find(x => x.id === state.machine);
  const shown = state.chosen?.path
    ?? (state.machine ? defaultFolder(bases.get(state.machine), m?.env?.os, state.repo) : null);
  const path = el("div", shown ? "ip-path" : "ip-path dim",
    shown || (state.machine ? "The machine's projects folder" : "The default folder on the machine Beep picks"));
  if (shown) path.title = shown;
  const pick = button(reading ? "Reading…" : "Choose…", "dbtn quiet", choose);
  pick.disabled = !machines.length || reading;
  const reset = state.chosen ? button("Default", "dbtn quiet", () => update({ type: "unchoose" })) : null;
  return el("div", "am-line", path, pick, reset);
}

/// The folder browser for another machine: the folder, its folders, and
/// what importing there does.
function browser(listing) {
  const up = button("Up", "dbtn quiet ip-up", () => read(listing.parent, l => update({ type: "browse", listing: l })));
  up.disabled = !listing.parent || reading;
  const at = el("div", "at", up, el("span", "", listing.path));
  at.lastChild.title = listing.path;
  const into = name => `${listing.path.replace(/\/+$/, "")}/${name}`;
  const dirs = el("div", "dirs", ...listing.folders.map(name => {
    const b = button(name, "", () => read(into(name), l => update({ type: "browse", listing: l })));
    b.disabled = reading;
    return b;
  }));
  if (!listing.folders.length) dirs.append(el("span", "none", listing.exists ? "No folders in here" : "Not there yet"));
  if (listing.more) dirs.append(el("span", "none", `and ${listing.more} more`));
  const use = button("Use this folder", "dbtn pri", () => update({ type: "choose", listing }));
  const cancel = button("Cancel", "dbtn quiet", () => update({ type: "browse", listing: null }));
  const foot = el("div", "foot", el("span", "note", folderNote(listing, state.repo)), cancel, use);
  return el("div", "ip-browse", at, dirs, foot);
}

/// Loads the sheet's styles once: the Add a machine sheet's, plus the field.
function injectStyle() {
  if (!document.querySelector("link[data-am]")) {
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.href = new URL("./addmachine.css", import.meta.url).href;
    link.dataset.am = "";
    document.head.append(link);
  }
  if (document.querySelector("style[data-ip]")) return;
  const style = document.createElement("style");
  style.dataset.ip = "";
  style.textContent = ".ip-field { flex: 1; min-width: 0; padding: 8px 11px; border-radius: 8px; border: 1px solid var(--border); background: #0b100e; color: var(--fg); font: inherit; font-size: 13px; outline: 0; caret-color: var(--emerald) } .ip-field:focus { border-color: var(--emerald) } .ip-sheet .am-sub { margin-top: 0 }"
    + " .ip-path { flex: 1; min-width: 0; padding: 7px 11px; border-radius: 8px; border: 1px solid var(--border); background: #0b100e; font-family: var(--mono); font-size: 12.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }"
    + " .ip-path.dim { color: var(--muted); font-family: inherit }"
    + " .ip-browse { border: 1px solid var(--border); border-radius: 10px; background: #0b100e; overflow: hidden }"
    + " .ip-browse .at { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-bottom: 1px solid var(--border); font-family: var(--mono); font-size: 12.5px }"
    + " .ip-browse .at span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }"
    + " .ip-browse .dirs { max-height: 168px; overflow: auto; padding: 4px 0; display: flex; flex-direction: column }"
    + " .ip-browse .dirs button { all: unset; box-sizing: border-box; width: 100%; padding: 5px 12px; font-size: 13px; cursor: pointer }"
    + " .ip-browse .dirs button:hover, .ip-browse .dirs button:focus-visible { background: rgba(255,255,255,.05) }"
    + " .ip-browse .dirs .none { padding: 5px 12px; font-size: 12.5px; color: var(--muted) }"
    + " .ip-browse .foot { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-top: 1px solid var(--border); font-size: 12px; color: var(--muted) }"
    + " .ip-browse .foot .note { flex: 1; min-width: 0 }"
    + " .ip-found { display: flex; gap: 8px; align-items: center; font-size: 12.5px }"
    + " .ip-branches { display: flex; flex-direction: column; gap: 2px; max-height: 200px; overflow: auto }"
    + " .ip-branch { display: flex; flex-wrap: wrap; gap: 4px 8px; align-items: center; padding: 5px 0; font-size: 12.5px; border-bottom: 1px solid var(--border) }"
    + " .ip-branch:last-child { border-bottom: 0 }"
    + " .ip-branch code { font-family: var(--mono) } .ip-branch .m { flex: 1; min-width: 0; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap }"
    + " .ip-branch .lbl { color: var(--muted); font-size: 11.5px }"
    + " .ip-branch .seg { display: inline-flex; border: 1px solid var(--border); border-radius: 7px; overflow: hidden }"
    + " .ip-branch .seg button { all: unset; cursor: pointer; padding: 3px 9px; font-size: 12px } .ip-branch .seg button + button { border-left: 1px solid var(--border) }"
    + " .ip-branch .seg button[aria-pressed=true] { background: rgba(52, 211, 153, .14); color: var(--fg) }";
  document.head.append(style);
}
