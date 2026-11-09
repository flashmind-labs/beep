import { createElement, Fragment } from "react";
import { flushSync } from "react-dom";
import { openBarPage } from "../bar/page-root.ts";
import { AgentNewShell } from "./agentnew-shell.tsx";
import { stashBar } from "../chat/stash.ts";
import { startAttachments } from "./start-attachments.ts";
import { makeStartComposer } from "./start-composer.ts";
import { FolderChoice, NewAgentNotes, ProjectChoice } from "./start-composer-view.tsx";
import { ChoiceMenuView, type ChoiceMenu, type ChoiceRow } from "./choice-menu-view.tsx";
import { BaseChoice, BaseInput, FolderInput } from "./choice-extra-view.tsx";
import { AG, AGENT_ORDER, agentKind, byPreference, loadAgentHosts, renderAgents, type Started } from "./agents.ts";
import { loadSessions, sessions } from "./sessions.ts";
import { openAgent, sessionPicks } from "./session.ts";
import { closedPicker, drawPicker as drawPickerIn, focusSearch, knownModels, modelsOn, type Picked, pillInner } from "./agentpicker.ts";
import { type BaseBranches, baseFor, baseRows, type CheckoutPick, DEFAULT_CHECKOUT, folderPill, folderRows, type FormProject, formProjects, modelId, harnessFor, keepEffort, knownWorktrees, machineRows, placeMenuAt, makesBranch, projectAt, projectFolders, startPlace, tildePath } from "./agentpick.ts";
import { $, $$, type AgentReturn, bar, PANE, S } from "../bar/core.ts";
import { completed, type FolderMenu, isOpen, menuKey, menuOf, NO_MENU, splitTyped, suggest } from "../addproject/foldercomplete.ts";
import { folderOn, type MachineFolder, type ProjectRow } from "../project/projects-state.ts";
import { addHosts, barCheckout, barPlace } from "../project/projectbar.ts";
import type { CachedViews } from "../center/cached.ts";
import type { HomeView } from "../center/home-state.ts";
import { composeField, composeMessage } from "../chat/compose.ts";
import type { Machine } from "../machines/machines.ts";
import { invoke } from "../ui/tauri.ts";

// ----- Start form -----

// The project composer, with a project selector when opened outside a project.

/// The machine and folder a start form is preset to, as a project's
/// "Start an agent here" gives them.
export interface AgentPlace {
  machine?: string | null;
  cwd?: string | null;
  /// The project's checkout on each machine.
  places?: { machine: string; cwd: string }[];
  /// The project's name, for the heading.
  project?: string | null;
}

/// The machine last picked for each project ("" for none), so the form
/// opens on it next time.
const LAST_MACHINE = "beep.lastMachine.v1";

function lastMachines(): Record<string, string> {
  try {
    const kept = JSON.parse(localStorage.getItem(LAST_MACHINE) ?? "{}");
    return kept && typeof kept === "object" ? kept : {};
  } catch {
    return {};
  }
}

/// The machine last picked for `project`, or "".
export function lastMachine(project: string | null | undefined) {
  const id = lastMachines()[project ?? ""];
  return typeof id === "string" ? id : "";
}

export function keepMachine(project: string | null | undefined, machine: string) {
  if (!machine) return;

  try {
    localStorage.setItem(LAST_MACHINE, JSON.stringify({ ...lastMachines(), [project ?? ""]: machine }));
  } catch {
    // Only a convenience: the form then opens without a machine.
  }
}

/// Every machine of the user's, online or not, for the Machine menu; null
/// until read.
let KNOWN: Machine[] | null = null;
let composer: ReturnType<typeof makeStartComposer>;
let attachments: ReturnType<typeof startAttachments>;
let fixedProject = false;

/// Each project's branches for the Base menu, by project: `undefined`
/// while asked, `null` when the server cannot list them.
const BRANCHES = new Map<string, BaseBranches | null | undefined>();

/// Reads `project`'s branches once per form, then redraws.
function loadBranches(project: string) {
  BRANCHES.set(project, undefined);
  invoke<BaseBranches | null>("project_branches", { project })
    .catch(() => null)
    .then(list => { BRANCHES.set(project, list); if (S.view === "agentnew" && AG.form.project === project) drawParts(); });
}

const formBranches = () => AG.form.project ? BRANCHES.get(AG.form.project) : null;

/// The user's projects and their checkouts, for the Project and Folder
/// menus; null until read.
let PROJECTS: FormProject[] | null = null;

/// Reads the projects: Home as the app kept it, at once, then the server's
/// answer when nothing was kept.
async function loadProjects() {
  const show = (rows: ProjectRow[] | null | undefined) => {
    if (!rows) return false;
    PROJECTS = formProjects(rows);
    if (S.view === "agentnew") drawParts();
    return true;
  };
  const kept = await invoke<CachedViews | null>("center_cached").catch(() => null);
  if (show(kept?.home?.projects)) return;
  const home = await invoke<HomeView>("center_home").catch(e => { console.warn("projects for the start form:", e); return null; });
  show(home?.projects);
}

/// The start form with `prompt`; `from` is where Esc goes back to.
/// `at` presets the machine and folder, as a project's "Start an agent here" does.
export function renderAgentNew(prompt = "", from: AgentReturn = "chat", at: AgentPlace | null = null) {
  const chat = S.view === "chat" ? S.session : null;
  S.view = "agentnew";
  AG.back = from;
  AG.form = { prompt, machine: at?.machine || lastMachine(at?.project), agent: null, cwd: at?.cwd || "", places: at?.places, chat, project: at?.project ?? null, checkout: DEFAULT_CHECKOUT, base: null, baseTyping: false, effort: null, open: null, query: "", typing: false };
  AG.error = "";
  AG.busy = false;
  fixedProject = !!at?.project;
  drawFrame();
  $("#agprompt")?.focus();
  loadAgentHosts();
  loadSessions().then(() => { if (S.view === "agentnew") drawParts(); }).catch(() => {});
  invoke<Machine[]>("list_machines").then(list => { KNOWN = list.filter(m => m.can_host !== false); if (S.view === "agentnew") drawParts(); }).catch(() => {});
  if (at?.project) loadBranches(at.project);
  loadProjects();
}

/// The same project task hint, or a general one before a project is picked.
function taskHint(missing: boolean) {
  if (missing) return "Say what the agent should do.";
  return "Describe the task.";
}

export function formHost() { return AG.hosts?.find(m => m.machine_id === AG.form.machine) ?? null; }
/// The agents the chosen machine offers; before one is picked, any machine's.
export function formAgents() {
  const host = formHost();
  const all = host ? host.agents : [...new Set((AG.hosts ?? []).flatMap(m => m.agents))];
  return all.sort(byPreference);
}

/// Redraws the form, keeping the prompt as typed when it shows already.
export function drawAgentNew() {
  if ($("#agbox")) drawParts();
  else drawFrame();
}

/// The form's frame: heading, prompt and the slots `drawParts` fills. The
/// prompt is drawn once, so redrawing the rest keeps what is typed.
function drawFrame() {
  const f = AG.form;
  composer = makeStartComposer({ projectPicker: !fixedProject });
  const c = composer;
  attachments = startAttachments(c, text => { AG.error = text; c.err.textContent = text; }, () => drawParts());
  c.box.id = "agbox";
  c.input.id = "agprompt";
  c.input.value = f.prompt;
  c.choices.id = "agchoices";
  c.project.id = "agprojb";
  c.machine.id = "aghostb";
  c.folder.id = "agfoldb";
  c.harness.id = "agpillb";
  c.send.id = "aggo";
  c.pick.id = "agpick";
  c.err.id = "agerr";
  const { root } = openBarPage(() => { closedPicker(c.pick); c.dispose(); });
  flushSync(() => root.render(createElement(AgentNewShell, { pane: PANE, composer: c.form, stash: stashBar(f.project ?? null) })));
  c.form.addEventListener("submit", e => { e.preventDefault(); void startAgent(); });
  c.input.addEventListener("input", () => {
    f.prompt = c.input.value;
    if (AG.error === "prompt") { AG.error = ""; drawParts(); }
  });
  composeField(c.input, { images: files => attachments.add(files) });
  attachments.bindStash(() => AG.form.project ?? null);
  bar.onclick = onClick;
  drawParts();
}

/// Brings the form's choices in line with the machine picked, then draws
/// everything but the prompt.
function drawParts() {
  if (S.view !== "agentnew" || !$("#agbox")) return;
  const f = AG.form, hosts = AG.hosts ?? [];
  const stashKeys = bar.querySelector(".prompt-stash-bar");
  if (stashKeys) stashKeys.outerHTML = stashBar(f.project ?? null);

  // With no machine picked, or one that no longer runs agents: the project's
  // first machine with a checkout and an agent, else the first with an agent.
  if (AG.hosts && !formHost()) {
    f.machine = (f.places && barPlace(f.places, AG.hosts, "")?.machine_id) || addHosts(AG.hosts)[0]?.machine_id || "";
    if (f.machine) f.cwd = folderOn(f.cwd, f.machine, f.places);
  }

  f.agent = harnessFor(f.agent, formAgents(), AGENT_ORDER);
  f.effort = keepEffort(f.agent, f.effort);
  syncModel();
  const none = !!AG.hosts && !hosts.length;
  const missing = AG.error === "prompt";
  $$("#agbox").classList.toggle("err", missing);
  $$<HTMLTextAreaElement>("#agprompt").placeholder = taskHint(missing);
  // Keep focus when the picker or typed folder is drawn again.
  const active = document.activeElement;
  const focused = active?.closest("#agpick") ? "#agpick .agc-search" : active?.id ? `#${active.id}` : "";
  const go = $$<HTMLButtonElement>("#aggo");
  go.disabled = AG.busy || attachments.reading || !hosts.length || !f.machine || !f.project;
  go.title = !f.project ? "Pick a project first" : !f.machine ? "Pick a machine first" : "Start (⌘↩)";
  // The key hints give way to an open menu.
  bar.classList.toggle("agc-menuopen", !!f.open);
  composer.setBusy(AG.busy);
  attachments.setBusy(AG.busy);
  requestAnimationFrame(composer.grow);
  drawPill(none);
  drawPicker();
  drawUnder();
  composer.setNotes(createElement(NewAgentNotes, { waiting: !AG.hosts, none, noMachine: !f.machine && !!AG.hosts?.length }));
  composer.err.textContent = missing ? "Say what the agent should do." : AG.error;
  if ((focused === "#agpick .agc-search" || focused === "#agcwd" || focused === "#agbase") && document.activeElement !== $(focused)) focusEnd(focused);
}

/// Focuses the field at `sel` with the caret at its end.
function focusEnd(sel: string) {
  const field = $<HTMLInputElement>(sel);
  field?.focus();
  field?.setSelectionRange(field.value.length, field.value.length);
}

function drawPill(none: boolean) {
  const f = AG.form, button = composer.harness;
  button.disabled = none || !f.agent;
  button.setAttribute("aria-expanded", String(f.open === "pick"));
  button.innerHTML = none || !f.agent ? "No agent" : pillInner(f.agent, f.model ?? "", f.effort ?? null, f.modelFor ? f.machine || null : null);
}

/// The picker by the pill: harnesses, their models, their thinking.
function drawPicker() {
  const f = AG.form, el = $$("#agpick");
  if (f.open !== "pick" || !f.agent) { closedPicker(el); el.innerHTML = ""; return; }
  const host = formHost(), machine = f.modelFor ? f.machine || null : null;
  const name = machine ? AG.hosts?.find(m => m.machine_id === machine)?.machine_name ?? machine : null;
  const picked: Picked = { agent: f.agent, model: f.model ?? "", effort: f.effort ?? null, query: f.query ?? "" };
  drawPickerIn(el, {
    picked,
    offered: formAgents(),
    everywhere: [...new Set((AG.hosts ?? []).flatMap(m => m.agents))],
    machine: machine && name ? { id: machine, name: host?.machine_name ?? name } : null,
    noMachine: "Pick a machine to list its models.",
    anchor: () => $("#agpillb"),
    bounds: () => bar,
    changed: what => {
      if (what === "models") return drawPill(false);
      Object.assign(f, { agent: picked.agent, model: picked.model, effort: picked.effort, query: picked.query });
      if (what === "model") { f.open = null; drawParts(); return $("#agprompt")?.focus(); }
      drawParts();
      if (what === "agent") focusSearch(el);
    },
  });
}

addEventListener("resize", () => { if (S.view === "agentnew") placeMenu(); });
// The menu floats over the page, so it follows its pill when the form scrolls.
addEventListener("scroll", () => { if (S.view === "agentnew") placeMenu(); }, { capture: true, passive: true });

/// Project, machine and checkout controls, with whichever menu is open.
function drawUnder() {
  const f = AG.form, host = formHost();
  const checkout = checkoutHere();
  const shown = folderPill(f.cwd, host?.workspace, host?.home);
  const co = barCheckout(f.checkout ?? DEFAULT_CHECKOUT, baseFor(f.base, formBranches()));
  const c = composer;
  c.folder.hidden = !!f.typing;
  c.folder.title = f.cwd.trim() || host?.workspace || "The machine's workspace";
  c.folder.setAttribute("aria-expanded", String(f.open === "folder"));
  drawExtras();
  c.project.title = f.project ? "The project this agent works on" : "Pick a project to start";
  c.project.setAttribute("aria-expanded", String(f.open === "project"));
  c.machine.setAttribute("aria-expanded", String(f.open === "host"));
  c.setChoiceContents({ project: createElement(ProjectChoice, { name: f.project || "No project" }), machine: host ? host.machine_name : "Pick a machine", folder: createElement(FolderChoice, { checkout, label: checkout ? co.label : shown.label, detail: checkout ? co.sub : shown.path }) });
  c.setUnder(createElement(ChoiceMenuView, { menu: projectMenu() ?? hostMenu() ?? folderMenu(checkout) ?? baseMenu() }));
  placeMenu();
}

function drawExtras() {
  const f = AG.form, base = baseFor(f.base, formBranches()) || "HEAD";
  const folder = f.typing ? createElement(FolderInput, { value: f.cwd, menu, bind: bindFolderField, pick: pickFolder }) : null;
  const branch = checkoutHere() && makesBranch(f.checkout ?? DEFAULT_CHECKOUT)
    ? f.baseTyping ? createElement(BaseInput, { value: f.base ?? "", placeholder: base, bind: bindBaseField }) : createElement(BaseChoice, { base, open: f.open === "base" }) : null;
  composer.setExtras(createElement(Fragment, null, folder, branch));
}

/// Places the open menu by its pill, over the page so nothing clips it: below
/// when it fits, else above, scrolling inside.
function placeMenu() {
  const f = AG.form;
  const pill = f.open === "project" ? "#agprojb" : f.open === "host" ? "#aghostb" : f.open === "folder" ? "#agfoldb" : f.open === "base" ? "#agbaseb" : null;
  const menu = $<HTMLElement>("#agunder > .agc-menu"), anchor = pill ? $<HTMLElement>(pill) : null;
  if (!menu || !anchor) return;
  menu.classList.add("float");
  menu.style.maxHeight = "";
  const a = anchor.getBoundingClientRect(), view = bar.getBoundingClientRect();
  const at = placeMenuAt(a, { width: menu.offsetWidth, height: menu.scrollHeight },
    { top: Math.max(0, view.top), bottom: Math.min(innerHeight, view.bottom), width: innerWidth });
  menu.style.top = `${at.top}px`;
  menu.style.left = `${at.left}px`;
  menu.style.maxHeight = `${at.maxHeight}px`;
  menu.classList.toggle("up", at.up);
}

/// The Project menu: the user's projects, then none.
function projectMenu(): ChoiceMenu | null {
  const f = AG.form;
  if (f.open !== "project") return null;
  const machine = f.machine || null;
  const rows = (PROJECTS ?? []).map(p => {
    const here = machine ? p.places.find(x => x.machine === machine) : null;
    const where = here ? tildePath(here.cwd, formHost()?.home) : `${p.places.length} ${p.places.length === 1 ? "machine" : "machines"}`;
    return { kind: "button", action: "agp", value: p.name, selected: f.project === p.name, label: createElement("span", null, p.name), detail: where, mono: !!here } satisfies ChoiceRow;
  });
  const note: ChoiceRow[] = PROJECTS === null ? [{ kind: "button", action: null, value: "loading", disabled: true, label: createElement("span", null, createElement("span", { className: "dspin" }), " Reading projects…"), detail: "" }] : [];
  return { style: "", rows: [...note, ...rows, ...(rows.length || note.length ? [{ kind: "rule" } as const] : []), { kind: "button", action: "agp", value: "", selected: !f.project, label: "No project", detail: "pick one to start" }] };
}

/// Sets the form's project, on the machine last picked for it, else the one
/// picked when the project is there, else none.
function setProject(name: string | null) {
  const f = AG.form, p = name ? PROJECTS?.find(x => x.name === name) : null;
  f.checkout = DEFAULT_CHECKOUT;
  f.base = null;

  if (!p) {
    f.project = null;
    f.places = undefined;
    f.cwd = "";
    f.machine = lastMachine(null) || f.machine;
    return;
  }

  const last = lastMachine(p.name);
  if (last && p.places.some(x => x.machine === last)) f.machine = last;
  else if (f.machine && !p.places.some(x => x.machine === f.machine)) f.machine = "";
  f.project = p.name;
  f.places = p.places;
  f.cwd = folderOn(p.places[0]?.cwd ?? "", f.machine || null, p.places);
  if (!BRANCHES.has(p.name)) loadBranches(p.name);
}

/// After a folder is picked or typed: the project whose checkout it is,
/// else none.
function projectForFolder() {
  const f = AG.form, machine = f.machine || null;
  const p = PROJECTS ? projectAt(PROJECTS, machine, f.cwd) : null;

  if (p) {
    f.project = p.name;
    f.places = p.places;
    if (!BRANCHES.has(p.name)) loadBranches(p.name);
  } else if (PROJECTS || !f.cwd.trim()) {
    f.project = null;
    f.places = undefined;
  }
}

/// The Base menu: the project's branches, the default first, and a row to
/// type any branch, tag or commit.
function baseMenu(): ChoiceMenu | null {
  const f = AG.form;
  if (f.open !== "base") return null;
  const list = formBranches(), picked = baseFor(f.base, list);
  const rows: ChoiceRow[] = baseRows(list).map(name => ({ kind: "button", action: "agb", value: name, selected: picked === name, label: createElement("span", { className: "mono" }, name), detail: name === list?.default_branch ? "default" : "" }));
  const note: ChoiceRow[] = list === undefined ? [{ kind: "button", action: null, value: "loading", disabled: true, label: createElement("span", null, createElement("span", { className: "dspin" }), " Reading branches…"), detail: "" }] : [];
  return { style: "base", rows: [...note, ...rows, ...(rows.length || note.length ? [{ kind: "rule" } as const] : []), { kind: "button", action: "agbtype", value: "1", label: "Type a branch or commit…", detail: "" }] };
}

/// The base field: Enter or a click elsewhere keeps what is typed; empty
/// goes back to the default.
function bindBaseField() {
  const f = AG.form, input = $$<HTMLInputElement>("#agbase");
  input.addEventListener("input", () => { f.base = input.value.trim() || null; });
  input.addEventListener("keydown", e => {
    if (e.key === "Enter" && !e.metaKey) { e.preventDefault(); f.baseTyping = false; drawParts(); }
  });
  input.addEventListener("blur", () => setTimeout(() => { if (document.activeElement !== input && AG.form.baseTyping) { AG.form.baseTyping = false; drawParts(); } }, 150));
}

function hostMenu(): ChoiceMenu | null {
  const f = AG.form;
  if (f.open !== "host") return null;
  const rows = machineRows(AG.hosts ?? [], KNOWN?.map(m => ({ id: m.id, name: m.name, online: m.online })) ?? null, a => agentKind(a).name, AGENT_ORDER);
  return { style: "cols", rows: rows.map(m => ({ kind: "button", action: "agm", value: m.id, selected: f.machine === m.id, disabled: !m.enabled, dot: m.online ? "on" : "off", label: createElement("span", null, m.name), detail: m.note })) };
}

/// Whether the folder is one of the project's checkouts on the machine
/// picked (any of them before one is).
function checkoutHere() {
  const f = AG.form, cwd = f.cwd.trim();
  if (!cwd || !f.places?.length) return false;
  const machine = f.machine || null;
  return f.places.some(p => (!machine || p.machine === machine) && (p.cwd === cwd || folderOn(p.cwd, null, null) === cwd));
}

function folderMenu(checkout: boolean): ChoiceMenu | null {
  const f = AG.form;
  if (f.open !== "folder") return null;
  const machine = f.machine || null, host = formHost();

  if (!f.project) {
    // Outside a project: the workspace, then the projects checked out on
    // the machine; picking one sets the Project pill.
    const rows: ChoiceRow[] = folderRows(f.cwd, host?.workspace, host?.home, projectFolders(PROJECTS ?? [], machine, host?.home)).map(r => {
      if (r.kind === "rule") return { kind: "rule" };
      if (r.kind === "type") return { kind: "button", action: "agtype", value: "1", label: "Type a folder…", detail: "" };
      if (r.kind === "workspace") return { kind: "button", action: "agf", value: "", selected: r.on, label: "Workspace", detail: r.shown, mono: true };
      return { kind: "button", action: "agf", value: r.cwd, selected: r.on, label: createElement("span", null, r.project), detail: r.shown, mono: true };
    });
    return { style: "folder", rows };
  }

  const places = [...new Set((f.places ?? []).filter(p => !machine || p.machine === machine).map(p => machine ? p.cwd : folderOn(p.cwd, null, null)))];
  const pick = f.checkout ?? DEFAULT_CHECKOUT;
  const same = (p: CheckoutPick) => JSON.stringify(p) === JSON.stringify(pick);
  const trees = checkout ? knownWorktrees(sessions, f.machine || null, f.cwd) : [];
  const workspace = host?.workspace ? tildePath(host.workspace, host.home) : "~";
  const rows: ChoiceRow[] = [
    ...places.map(p => ({ kind: "button", action: "agf", value: p, selected: f.cwd.trim() === p, label: createElement("span", { className: "mono" }, tildePath(p, host?.home)), detail: "checkout" } as const)),
    ...(checkout ? [{ kind: "rule" } as const,
      { kind: "button", action: "agk", value: "current", selected: same("current"), label: "Current checkout", detail: "as it is" } as const,
      { kind: "button", action: "agk", value: "branch", selected: same("branch"), label: "New branch", detail: "in the checkout, named from the prompt" } as const,
      { kind: "button", action: "agk", value: "new", selected: same("new"), label: "New worktree", detail: "new branch in .worktrees/" } as const,
      ...trees.map(t => ({ kind: "button", action: "agw", value: t, selected: same({ worktree: t }), label: createElement("span", { className: "mono" }, t.split("/").slice(-2).join("/")), detail: "worktree" } as const))] : []),
    ...(places.length || checkout ? [{ kind: "rule" } as const] : []),
    { kind: "button", action: "agf", value: "", selected: !f.cwd.trim(), label: "Workspace", detail: workspace, mono: true },
    { kind: "button", action: "agtype", value: "1", label: "Type a folder…", detail: "" },
  ];
  return { style: "folder", rows };
}

function onClick(e: MouseEvent) {
  if (S.view !== "agentnew" || AG.busy) return;
  const f = AG.form, target = e.target as HTMLElement;
  const at = (sel: string) => target.closest<HTMLElement>(sel);
  if (at("#agList")) return renderAgents();
  if (at("#agmachines")) return import("../machines/machines.ts").then(m => m.renderMachines());
  if (at("#agpillb")) return togglePicker();
  if (at("#agprojb")) { f.open = f.open === "project" ? null : "project"; return drawParts(); }
  const project = at("[data-agp]");
  if (project) { setProject(project.dataset.agp || null); f.open = null; return drawParts(); }
  if (at("#aghostb")) { f.open = f.open === "host" ? null : "host"; return drawParts(); }
  if (at("#agfoldb")) { f.open = f.open === "folder" ? null : "folder"; return drawParts(); }

  if (at("#agbaseb")) {
    // Without the project's branches, the ref is typed.
    if (formBranches() === null) return typeBase();
    f.open = f.open === "base" ? null : "base";
    return drawParts();
  }

  const base = at("[data-agb]");
  if (base) { f.base = base.dataset.agb!; f.open = null; return drawParts(); }
  if (at("[data-agbtype]")) return typeBase();
  const machine = at("[data-agm]");

  if (machine) {
    f.machine = machine.dataset.agm!;
    keepMachine(f.project, f.machine);
    f.cwd = folderOn(f.cwd, f.machine, f.places);
    f.open = null;
    return drawParts();
  }

  const folder = at("[data-agf]");
  if (folder) { f.cwd = folder.dataset.agf!; f.checkout = DEFAULT_CHECKOUT; f.open = null; projectForFolder(); return drawParts(); }
  const checkout = at("[data-agk]");
  if (checkout) { f.checkout = checkout.dataset.agk as CheckoutPick; f.open = null; return drawParts(); }
  const tree = at("[data-agw]");
  if (tree) { f.checkout = { worktree: tree.dataset.agw! }; f.open = null; return drawParts(); }

  if (at("[data-agtype]")) {
    f.typing = true;
    f.open = null;
    drawParts();
    return focusEnd("#agcwd");
  }

  // A click anywhere else closes what is open.
  if (f.open && !at(".agc-pick, .agc-menu")) { f.open = null; drawParts(); }
}

/// Turns the Base button into a field to type a ref in.
function typeBase() {
  const f = AG.form;
  f.baseTyping = true;
  f.open = null;
  drawParts();
  focusEnd("#agbase");
}

/// Opens or closes the picker; open, the search takes the keys.
export function togglePicker() {
  const f = AG.form;
  if (S.view !== "agentnew" || !f.agent) return;
  f.open = f.open === "pick" ? null : "pick";
  f.query = "";
  drawParts();
  if (f.open === "pick") focusSearch($$("#agpick"));
}

/// Closes an open menu or the folder field; true when there was one, so
/// Esc goes no further.
export function closeAgentNewMenus() {
  const f = AG.form;
  if (S.view !== "agentnew" || (!f.open && !f.typing && !f.baseTyping)) return false;
  f.open = null;
  f.typing = false;
  f.baseTyping = false;
  drawParts();
  $("#agprompt")?.focus();
  return true;
}

// ----- Typing a folder -----

let menu: FolderMenu = NO_MENU;
let lookup = 0;
let lookupTimer: ReturnType<typeof setTimeout> | undefined;
const listed = new Map<string, string[]>();

/// The folder field: typing sets the folder, the menu offers the machine's
/// folders under what is typed, Enter or a click elsewhere keeps it.
function bindFolderField() {
  const f = AG.form, input = $$<HTMLInputElement>("#agcwd");
  input.addEventListener("input", () => { f.cwd = input.value; f.checkout = DEFAULT_CHECKOUT; lookFolders(input); });
  input.addEventListener("keydown", e => {
    const act = menuKey(menu, e.key, e.shiftKey || e.altKey || e.ctrlKey || e.metaKey);

    if (act.type === "none") {
      if (e.key === "Enter" && !e.metaKey) { e.preventDefault(); doneTyping(); }
      return;
    }

    e.preventDefault();
    e.stopPropagation();

    if (act.type === "move") showMenu(input, { ...menu, index: act.index });
    else if (act.type === "close") showMenu(input, NO_MENU);
    else { f.cwd = input.value = act.text; lookFolders(input); }
  });
  input.addEventListener("blur", () => setTimeout(() => { if (document.activeElement !== input && AG.form.typing) doneTyping(); }, 150));
  lookFolders(input);
}

function doneTyping() {
  const f = AG.form;
  f.typing = false;
  projectForFolder();
  menu = NO_MENU;
  lookup++;
  clearTimeout(lookupTimer);
  drawParts();
}

/// Offers the folders under what is typed, on the machine picked; nothing
/// before one is.
function lookFolders(input: HTMLInputElement) {
  clearTimeout(lookupTimer);
  const ask = ++lookup, machine = AG.form.machine || null, typed = splitTyped(input.value);
  if (!machine || !typed) return showMenu(input, NO_MENU);
  const key = `${machine}\n${typed.dir}`;
  const offer = (folders: readonly string[]) => { if (ask === lookup) showMenu(input, menuOf(typed.dir, suggest(folders, typed.prefix))); };
  const known = listed.get(key);
  if (known) return offer(known);
  lookupTimer = setTimeout(async () => {
    try {
      const listing = await invoke<MachineFolder>("list_machine_folder", { machineId: machine, path: typed.dir });
      listed.set(key, listing.folders);
      offer(listing.folders);
    } catch {
      if (ask === lookup) showMenu(input, NO_MENU);
    }
  }, 150);
}

function showMenu(input: HTMLInputElement, next: FolderMenu) {
  menu = document.activeElement === input ? next : NO_MENU;
  if ($("#agcwd") === input) drawExtras();
}

function pickFolder(index: number) {
  const input = $<HTMLInputElement>("#agcwd");
  if (!input || !isOpen(menu)) return;
  AG.form.cwd = input.value = completed(menu.dir, menu.items[index]);
  lookFolders(input);
}

// ----- Models -----

/// Keeps the model with the machine and harness it was picked for, and
/// asks for their models when not known lately.
function syncModel() {
  const f = AG.form, machine = f.machine || null;
  const key = machine && f.agent ? `${machine}/${f.agent}` : "";
  if (f.modelFor !== key) { f.model = ""; f.modelFor = key; }
  if (!key) return;
  modelsOn(machine!, f.agent!, () => { if (S.view === "agentnew" && AG.form.modelFor === key) drawPill(false); });
}

// ----- Start -----

export async function startAgent() {
  const f = AG.form;
  if (S.view !== "agentnew" || AG.busy || attachments.reading) return;
  if (!f.project) { f.open = "project"; return drawParts(); }
  if (!f.prompt.trim() && !attachments.count) { AG.error = "prompt"; drawParts(); return $("#agprompt")?.focus(); }
  if (!f.machine) { f.open = "host"; return drawParts(); }
  AG.busy = true;
  AG.error = "";
  f.open = null;
  drawParts();

  try {
    const models = knownModels(f.machine, f.agent);
    const model = f.model?.trim() && f.modelFor ? modelId(models, f.model) : null;
    const effort = f.effort || null;
    keepMachine(f.project, f.machine);
    // In a project's checkout, an existing worktree is a folder to start
    // in, and the node makes a new branch or worktree before it starts
    // only when one is picked.
    const { cwd, checkout } = checkoutHere()
      ? startPlace(f.checkout ?? DEFAULT_CHECKOUT, f.cwd.trim(), baseFor(f.base, formBranches()))
      : { cwd: f.cwd.trim(), checkout: null };
    const prompt = await attachments.files.prompt(composeMessage(f.prompt), f.machine, f.cwd.trim(), text => { composer.err.textContent = text; });
    const images = attachments.images.map(({ name, mime, data }) => ({ name, mime, data }));
    const started = await invoke<Started>("agent_start", {
      machineId: f.machine,
      agent: f.agent,
      cwd: cwd || null,
      prompt,
      images,
      chat: f.chat,
      model,
      effort,
      checkout,
    });
    const resolved = model ?? models?.current ?? "";
    sessionPicks.set(started.session.id, { model: resolved.toLowerCase() === "default" ? "" : resolved, effort });
    AG.busy = false;
    if (S.view !== "agentnew") return;
    // Back from the session goes where the form came from.
    openAgent(started.session, { why: started.why, back: AG.back });
  } catch (e) {
    AG.busy = false;
    AG.error = String(e).replace(/^bad request: /, "");
    if (S.view === "agentnew") drawParts();
  }
}
