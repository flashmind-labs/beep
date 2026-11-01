// The "Import a project" sheet: one field for org/repo or a URL, a chip per
// online machine (Auto lets the server pick), then Import. It shows the
// clone's progress and closes onto the new project. Esc or Cancel closes
// it, except while cloning, when the clone goes on and the project shows
// up once it is in.
import { START, next, looksLikeRepo, machineChips, progressText, doneText } from "./projects-state.js";

const { invoke } = window.__TAURI__.core;

let root = null, sheet = null, state = START, machines = [], back = null, onDone = null;

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

/// Opens the sheet. `options.machines` are the machines Home lists;
/// `options.onDone(project)` runs once a project is in, as when the window
/// selects it.
export function openImportProject(options = {}) {
  if (root) return;
  machines = options.machines || [];
  onDone = options.onDone || null;
  back = document.activeElement;
  injectStyle();
  state = START;
  root = el("div", "am-scrim");
  root.addEventListener("mousedown", e => { if (e.target === root && state.step !== "cloning") closeImportProject(); });
  sheet = el("div", "am-sheet ip-sheet");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", "Import a project");
  root.append(sheet);
  document.body.append(root);
  document.addEventListener("keydown", onKey, true);
  draw();
}

export function closeImportProject() {
  if (!root) return;
  document.removeEventListener("keydown", onKey, true);
  root.remove();
  root = sheet = null;
  const done = state.step === "done" ? state.project : null;
  state = START;
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
  try {
    const project = await invoke("import_project", { repo: state.repo, machineId: state.machine });
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
  sheet.replaceChildren(head, ...body());
  const field = sheet.querySelector("input");
  if (field) { field.focus(); field.setSelectionRange(field.value.length, field.value.length); }
  else sheet.querySelector(".dbtn.pri")?.focus();
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
  input.addEventListener("input", () => update({ type: "edit", repo: input.value }));
  input.addEventListener("keydown", e => { if (e.key === "Enter") { e.preventDefault(); submit(); } });
  const chips = el("div", "dchips", ...machineChips(machines).map(c => {
    const b = button(c.label, "dchip", () => update({ type: "pick", machine: c.id }));
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
    el("div", "am-sub", "On Linux it goes to projects/org/repo in the workspace; on a Mac, to ~/Projects/org/repo. Private repositories use your tokens from Settings > Credentials."),
    state.error ? el("div", "am-msg bad", state.error) : null,
    el("div", "am-line", go, button("Cancel", "dbtn quiet", closeImportProject)),
  ];
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
  style.textContent = ".ip-field { flex: 1; min-width: 0; padding: 8px 11px; border-radius: 8px; border: 1px solid var(--border); background: #0b100e; color: var(--fg); font: inherit; font-size: 13px; outline: 0; caret-color: var(--emerald) } .ip-field:focus { border-color: var(--emerald) } .ip-sheet .am-sub { margin-top: 0 }";
  document.head.append(style);
}
