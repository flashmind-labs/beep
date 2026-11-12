import { keepProjectPick, lastProjectPick } from "./agentmodels.ts";
import { Spinner } from "../ui/kit.tsx";
import { createElement, createRef, Fragment, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { flushSync } from "react-dom";
import { type BarPageSlot, openBarPage } from "../bar/bar-app.tsx";
import { startAttachments } from "./start-attachments.ts";
import { makeStartComposer, type StartComposer } from "./start-composer.ts";
import { menuPlaceFor } from "./mentionmenu.ts";
import { FolderChoice, NewAgentNotes, ProjectChoice } from "./start-composer-view.tsx";
import { ChoiceMenuView, type ChoiceAction, type ChoiceMenu, type ChoiceRow } from "./choice-menu-view.tsx";
import { BaseChoice, BaseInput, FolderInput } from "./choice-extra-view.tsx";
import { AG, AGENT_ORDER, agentKind, byPreference, hostChanges, loadAgentHosts, renderAgents, type Started } from "./agents.ts";
import { ensureSessions, sessions } from "./sessions.ts";
import { openAgent, sessionPicks } from "./session.ts";
import { closedPicker, drawPicker as drawPickerIn, focusSearch, knownModels, modelsOn, type Picked, pillContent } from "./agentpicker.ts";
import { type BaseBranches, baseFor, baseRows, type CheckoutPick, DEFAULT_CHECKOUT, folderPill, folderRows, type FormProject, formProjects, modelId, harnessFor, keepEffort, knownWorktrees, machineRows, makesBranch, projectAt, projectFolders, startPlace, tildePath } from "./agentpick.ts";
import { type AgentReturn, bar, PANE, S } from "../bar/core.ts";
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
let composer: StartComposer | null = null;
let shellPage: BarPageSlot | null = null;
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
  restoreProjectPick();
  AG.error = "";
  AG.busy = false;
  fixedProject = !!at?.project;
  drawFrame();
  composer?.input.focus();
  // What the form showed last stands until machines or accounts change.
  const changed = !AG.hosts || hostChanges.dirty("");
  if (changed) loadAgentHosts();
  ensureSessions().then(() => { if (S.view === "agentnew") drawParts(); }).catch(() => {});
  if (changed || !KNOWN) invoke<Machine[]>("list_machines").then(list => { KNOWN = list.filter(m => m.can_host !== false); if (S.view === "agentnew") drawParts(); }).catch(() => {});
  if (at?.project) loadBranches(at.project);
  loadProjects();
}

/// The same project task hint, or a general one before a project is picked.
function taskHint(missing: boolean) {
  if (missing) return "Say what the agent should do.";
  return "Describe the task.";
}

function formHost() { return AG.hosts?.find(m => m.machine_id === AG.form.machine) ?? null; }
/// The agents the chosen machine offers; before one is picked, any machine's.
function formAgents() {
  const host = formHost();
  const all = host ? host.agents : [...new Set((AG.hosts ?? []).flatMap(m => m.agents))];
  return all.sort(byPreference);
}

/// Redraws the form, keeping the prompt as typed when it shows already.
export function drawAgentNew() {
  if (composer?.box.isConnected) drawParts();
  else drawFrame();
}

/// The form's frame: heading, prompt and the parts `drawParts` fills. The
/// prompt is drawn once, so redrawing the rest keeps what is typed.
function drawFrame() {
  const f = AG.form;
  const c = composer = makeStartComposer({
    projectPicker: !fixedProject, ids: true,
    actions: {
      submit: () => void startAgent(), harness: () => { if (!AG.busy) togglePicker(); }, choice: toggleChoice, files: files => attachments.take(files),
      input: value => { AG.form.prompt = value; if (AG.error === "prompt") { AG.error = ""; drawParts(); } },
    },
    place: () => {
      const f = AG.form;
      return S.view === "agentnew" && f.machine && f.agent && f.cwd ? menuPlaceFor(f.machine, f.agent, f.cwd) : null;
    },
  });
  attachments = startAttachments(c, text => { AG.error = text; c.update({ error: text }); }, () => drawParts());
  c.setText(f.prompt);
  const page = shellPage = openBarPage(() => { shellPage = null; closedPicker(c.pick); c.dispose(); stopOutside(); });
  flushSync(() => page.show({ view: "agent-new", props: { pane: PANE, composer: c.form, project: f.project ?? null, menuOpen: !!f.open, openList: renderAgents } }));
  composeField(c.input, { images: files => attachments.add(files) });
  attachments.bindStash(() => AG.form.project ?? null);
  stopOutside();
  document.addEventListener("click", closeOutside);
  drawParts();
}

/// A click outside the open menu or picker closes it.
function closeOutside(e: MouseEvent) {
  const f = AG.form;
  if (S.view !== "agentnew" || AG.busy || !f.open || !(e.target instanceof Element)) return;
  if (e.target.closest(".agc-pick, .agc-menu, .pill")) return;
  f.open = null;
  drawParts();
}

function stopOutside() { document.removeEventListener("click", closeOutside); }

/// Brings the form's choices in line with the machine picked, then draws
/// everything but the prompt.
function drawParts() {
  const c = composer;
  if (S.view !== "agentnew" || !c?.box.isConnected) return;
  const f = AG.form, hosts = AG.hosts ?? [];
  const page = shellPage;
  // The key hints under the form give way to an open menu.
  if (page) flushSync(() => page.show({ view: "agent-new", props: { pane: PANE, composer: c.form, project: f.project ?? null, menuOpen: !!f.open, openList: renderAgents } }));

  // With no machine picked, or one that no longer runs agents: the project's
  // first machine with a checkout and an agent, else the first with an agent.
  if (AG.hosts && !formHost()) {
    f.machine = (f.places && barPlace(f.places, AG.hosts, "")?.machine_id) || addHosts(AG.hosts)[0]?.machine_id || "";
    if (f.machine) f.cwd = folderOn(f.cwd, f.machine, f.places);
  }

  const preferred = !f.modelFor ? lastProjectPick(f.project ?? null)?.agent : null;
  f.agent = harnessFor(preferred ?? f.agent, formAgents(), AGENT_ORDER);
  f.effort = keepEffort(f.agent, f.effort);
  syncModel();
  const none = !!AG.hosts && !hosts.length;
  const missing = AG.error === "prompt";
  const host = formHost();
  c.update({
    invalid: missing, placeholder: taskHint(missing), error: missing ? "Say what the agent should do." : AG.error,
    sendDisabled: AG.busy || attachments.reading || !hosts.length || !f.machine || !f.project,
    sendTitle: !f.project ? "Pick a project first" : !f.machine ? "Pick a machine first" : "Start (⌘↩)",
    busy: AG.busy, open: f.open ?? null,
    projectTitle: f.project ? "The project this agent works on" : "Pick a project to start",
    folderHidden: !!f.typing, folderTitle: f.cwd.trim() || host?.workspace || "The machine's workspace",
  });
  attachments.setBusy(AG.busy);
  // Text set from code (a stash restore, a start) fits once drawn.
  requestAnimationFrame(c.grow);
  drawPill(c, none);
  drawPicker(c);
  drawUnder(c);
  c.setNotes(createElement(NewAgentNotes, { waiting: !AG.hosts, none, noMachine: !f.machine && !!AG.hosts?.length, openMachines }));
}

function openMachines() { void import("../machines/machines.ts").then(m => m.renderMachines()); }

function drawPill(c: StartComposer, none: boolean) {
  const f = AG.form;
  c.update({ harnessDisabled: none || !f.agent });
  c.setHarness(none || !f.agent ? "No agent" : pillContent(f.agent, f.model ?? "", f.effort ?? null, f.modelFor ? f.machine || null : null));
}

/// The picker by the pill: harnesses, their models, their thinking.
function drawPicker(c: StartComposer) {
  const f = AG.form, el = c.pick;
  if (f.open !== "pick" || !f.agent) { closedPicker(el); return; }
  const host = formHost(), machine = f.modelFor ? f.machine || null : null;
  const name = machine ? AG.hosts?.find(m => m.machine_id === machine)?.machine_name ?? machine : null;
  const picked: Picked = { agent: f.agent, model: f.model ?? "", effort: f.effort ?? null, query: f.query ?? "" };
  drawPickerIn(el, {
    render: view => { if (c.form.isConnected) c.setPicker(view); },
    picked,
    offered: formAgents(),
    everywhere: [...new Set((AG.hosts ?? []).flatMap(m => m.agents))],
    machine: machine && name ? { id: machine, name: host?.machine_name ?? name } : null,
    noMachine: "Pick a machine to list its models.",
    anchor: () => c.harness.isConnected ? c.harness : null,
    bounds: () => bar,
    changed: what => {
      if (what === "models") return drawPill(c, false);
      Object.assign(f, { agent: picked.agent, model: picked.model, effort: picked.effort, query: picked.query });
      if (what !== "agent") keepProjectPick(f.project ?? null, picked);
      if (what === "model") { f.open = null; drawParts(); return c.input.focus(); }
      drawParts();
      if (what === "agent") {
        if (f.agent) keepProjectPick(f.project ?? null, { agent: f.agent, model: f.model ?? "", effort: f.effort ?? null });
        focusSearch(el);
      }
    },
  });
}

/// Project, machine and checkout controls, with whichever menu is open.
function drawUnder(c: StartComposer) {
  const f = AG.form, host = formHost();
  const checkout = checkoutHere();
  const shown = folderPill(f.cwd, host?.workspace, host?.home);
  const co = barCheckout(f.checkout ?? DEFAULT_CHECKOUT, baseFor(f.base, formBranches()));
  drawExtras(c);
  c.setChoiceContents({ project: createElement(ProjectChoice, { name: f.project || "No project" }), machine: host ? host.machine_name : "Pick a machine", folder: createElement(FolderChoice, { checkout, label: checkout ? co.label : shown.label, detail: checkout ? co.sub : shown.path }) });
  c.setUnder(createElement(ChoiceMenuView, { menu: projectMenu() ?? hostMenu() ?? folderMenu(checkout) ?? baseMenu(), anchor: menuAnchor, bounds: menuBounds, pick: pickChoice }));
}

const baseButton = createRef<HTMLButtonElement>();

function drawExtras(c: StartComposer) {
  const f = AG.form, base = baseFor(f.base, formBranches()) || "HEAD";
  const folder = f.typing ? createElement(FolderInput, { value: f.cwd, menu, change: typeFolder, keyDown: folderKey, leave: () => { if (AG.form.typing) doneTyping(); }, pick: pickFolder }) : null;
  const branch = checkoutHere() && makesBranch(f.checkout ?? DEFAULT_CHECKOUT)
    ? f.baseTyping
      ? createElement(BaseInput, { value: f.base ?? "", placeholder: base, change: value => { f.base = value.trim() || null; drawExtras(c); }, keyDown: baseKey, leave: () => { if (AG.form.baseTyping) { AG.form.baseTyping = false; drawParts(); } } })
      : createElement(BaseChoice, { base, open: f.open === "base", button: baseButton, toggle: toggleBase })
    : null;
  c.setExtras(createElement(Fragment, null, folder, branch));
}

/// The pill the open menu hangs from, and the area it stays in.
function menuAnchor() {
  const open = AG.form.open, c = composer;
  if (open === "base") return baseButton.current;
  return c && (open === "project" || open === "host" || open === "folder") ? c.choice(open) : null;
}

function menuBounds() { return bar; }

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
  const note: ChoiceRow[] = PROJECTS === null ? [{ kind: "button", action: null, value: "loading", disabled: true, label: createElement("span", null, createElement(Spinner), " Reading projects…"), detail: "" }] : [];
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
    restoreProjectPick();
    return;
  }

  const last = lastMachine(p.name);
  if (last && p.places.some(x => x.machine === last)) f.machine = last;
  else if (f.machine && !p.places.some(x => x.machine === f.machine)) f.machine = "";
  f.project = p.name;
  f.places = p.places;
  f.cwd = folderOn(p.places[0]?.cwd ?? "", f.machine || null, p.places);
  restoreProjectPick();
  if (!BRANCHES.has(p.name)) loadBranches(p.name);
}

/// After a folder is picked or typed: the project whose checkout it is,
/// else none.
function projectForFolder() {
  const f = AG.form, machine = f.machine || null;
  const previous = f.project;
  const p = PROJECTS ? projectAt(PROJECTS, machine, f.cwd) : null;

  if (p) {
    f.project = p.name;
    f.places = p.places;
    if (!BRANCHES.has(p.name)) loadBranches(p.name);
  } else if (PROJECTS || !f.cwd.trim()) {
    f.project = null;
    f.places = undefined;
  }

  if (f.project !== previous) restoreProjectPick();
}

/// The Base menu: the project's branches, the default first, and a row to
/// type any branch, tag or commit.
function baseMenu(): ChoiceMenu | null {
  const f = AG.form;
  if (f.open !== "base") return null;
  const list = formBranches(), picked = baseFor(f.base, list);
  const rows: ChoiceRow[] = baseRows(list).map(name => ({ kind: "button", action: "agb", value: name, selected: picked === name, label: createElement("span", { className: "mono" }, name), detail: name === list?.default_branch ? "default" : "" }));
  const note: ChoiceRow[] = list === undefined ? [{ kind: "button", action: null, value: "loading", disabled: true, label: createElement("span", null, createElement(Spinner), " Reading branches…"), detail: "" }] : [];
  return { style: "base", rows: [...note, ...rows, ...(rows.length || note.length ? [{ kind: "rule" } as const] : []), { kind: "button", action: "agbtype", value: "1", label: "Type a branch or commit…", detail: "" }] };
}

/// The base field: Enter keeps what is typed; empty goes back to the default.
function baseKey(e: ReactKeyboardEvent<HTMLInputElement>) {
  if (e.key === "Enter" && !e.metaKey) { e.preventDefault(); AG.form.baseTyping = false; drawParts(); }
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

/// Opens or closes the project, machine or folder menu.
function toggleChoice(which: "project" | "host" | "folder") {
  const f = AG.form;
  if (S.view !== "agentnew" || AG.busy) return;
  f.open = f.open === which ? null : which;
  drawParts();
}

function toggleBase() {
  const f = AG.form;
  if (S.view !== "agentnew" || AG.busy) return;
  // Without the project's branches, the ref is typed.
  if (formBranches() === null) return typeBase();
  f.open = f.open === "base" ? null : "base";
  drawParts();
}

/// A row picked in the open menu.
function pickChoice(action: ChoiceAction, value: string) {
  const f = AG.form;
  if (S.view !== "agentnew" || AG.busy) return;

  switch (action) {
    case "agp": setProject(value || null); break;
    case "agb": f.base = value; break;
    case "agbtype": return typeBase();
    case "agm":
      f.machine = value;
      keepMachine(f.project, f.machine);
      f.cwd = folderOn(f.cwd, f.machine, f.places);
      break;
    case "agf": f.cwd = value; f.checkout = DEFAULT_CHECKOUT; projectForFolder(); break;
    case "agk": if (isCheckoutKind(value)) f.checkout = value; break;
    case "agw": f.checkout = { worktree: value }; break;
    case "agtype":
      f.typing = true;
      f.open = null;
      drawParts();
      return lookFolders();
  }

  f.open = null;
  drawParts();
}

const isCheckoutKind = (value: string): value is "current" | "branch" | "new" => value === "current" || value === "branch" || value === "new";

/// Turns the Base button into a field to type a ref in.
function typeBase() {
  const f = AG.form;
  f.baseTyping = true;
  f.open = null;
  drawParts();
}

/// Opens or closes the picker; open, the search takes the keys.
export function togglePicker() {
  const f = AG.form;
  if (S.view !== "agentnew" || !f.agent) return;
  f.open = f.open === "pick" ? null : "pick";
  f.query = "";
  drawParts();
  if (f.open === "pick" && composer) focusSearch(composer.pick);
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
  composer?.input.focus();
  return true;
}

// ----- Typing a folder -----

let menu: FolderMenu = NO_MENU;
let lookup = 0;
let lookupTimer: ReturnType<typeof setTimeout> | undefined;
const listed = new Map<string, string[]>();

/// The folder field: typing sets the folder, the menu offers the machine's
/// folders under what is typed, Enter or a click elsewhere keeps it.
function typeFolder(value: string) {
  const f = AG.form;
  f.cwd = value;
  f.checkout = DEFAULT_CHECKOUT;
  if (composer) drawExtras(composer);
  lookFolders();
}

function folderKey(e: ReactKeyboardEvent<HTMLInputElement>) {
  const act = menuKey(menu, e.key, e.shiftKey || e.altKey || e.ctrlKey || e.metaKey);

  if (act.type === "none") {
    if (e.key === "Enter" && !e.metaKey) { e.preventDefault(); doneTyping(); }
    return;
  }

  e.preventDefault();
  e.stopPropagation();

  if (act.type === "move") showMenu({ ...menu, index: act.index });
  else if (act.type === "close") showMenu(NO_MENU);
  else typeFolder(act.text);
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
function lookFolders() {
  clearTimeout(lookupTimer);
  const ask = ++lookup, machine = AG.form.machine || null, typed = splitTyped(AG.form.cwd);
  if (!machine || !typed) return showMenu(NO_MENU);
  const key = `${machine}\n${typed.dir}`;
  const offer = (folders: readonly string[]) => { if (ask === lookup) showMenu(menuOf(typed.dir, suggest(folders, typed.prefix))); };
  const known = listed.get(key);
  if (known) return offer(known);
  lookupTimer = setTimeout(async () => {
    try {
      const listing = await invoke<MachineFolder>("list_machine_folder", { machineId: machine, path: typed.dir });
      listed.set(key, listing.folders);
      offer(listing.folders);
    } catch {
      if (ask === lookup) showMenu(NO_MENU);
    }
  }, 150);
}

function showMenu(next: FolderMenu) {
  menu = AG.form.typing ? next : NO_MENU;
  if (composer && S.view === "agentnew") drawExtras(composer);
}

function pickFolder(index: number) {
  if (!AG.form.typing || !isOpen(menu)) return;
  typeFolder(completed(menu.dir, menu.items[index]));
}

// ----- Models -----

function restoreProjectPick() {
  const f = AG.form, saved = lastProjectPick(f.project ?? null);
  f.agent = saved?.agent ?? null;
  f.model = saved?.model ?? "";
  f.effort = saved?.effort ?? null;
  f.modelFor = "";
}

/// Keeps the model with the machine and harness it was picked for, and
/// asks for their models when not known lately.
function syncModel() {
  const f = AG.form, machine = f.machine || null, agent = f.agent;
  const key = machine && agent ? `${machine}/${agent}` : "";
  if (f.modelFor !== key) {
    const saved = !f.modelFor ? lastProjectPick(f.project ?? null) : null;
    f.model = saved?.agent === agent ? saved.model : "";
    if (saved?.agent === agent) f.effort = keepEffort(agent, saved.effort);
    f.modelFor = key;
  }
  if (!machine || !agent) return;
  modelsOn(machine, agent, () => { if (S.view === "agentnew" && AG.form.modelFor === key && composer) drawPill(composer, false); });
}

// ----- Start -----

export async function startAgent() {
  const f = AG.form;
  if (S.view !== "agentnew" || AG.busy || attachments.reading) return;
  if (!f.project) { f.open = "project"; return drawParts(); }
  if (!f.prompt.trim() && !attachments.count) { AG.error = "prompt"; drawParts(); return composer?.input.focus(); }
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
    const prompt = await attachments.files.prompt(composeMessage(f.prompt), f.machine, f.cwd.trim(), progress => composer?.update({ progress }));
    if (attachments.files.pending.length) composer?.update({ progress: "Starting the agent…" });
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
    composer?.update({ progress: "" });
    if (S.view !== "agentnew") return;
    // Back from the session goes where the form came from.
    openAgent(started.session, { why: started.why, back: AG.back });
  } catch (e) {
    AG.busy = false;
    composer?.update({ progress: "" });
    AG.error = String(e).replace(/^bad request: /, "");
    if (S.view === "agentnew") drawParts();
  }
}
