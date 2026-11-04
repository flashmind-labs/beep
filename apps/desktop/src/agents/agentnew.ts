import { AG, AGENT_ORDER, agentKind, type AgentSession, byPreference, loadAgentHosts, renderAgents, type Started } from "./agents.ts";
import { openAgent } from "./session.ts";
import { closedPicker, drawPicker as drawPickerIn, focusSearch, modelsOn, type Picked, pillInner } from "./agentpicker.ts";
import { type BaseBranches, baseFor, baseRows, type CheckoutPick, checkoutLabel, DEFAULT_CHECKOUT, folderPill, folderRows, type FormProject, formProjects, formTitle, harnessFor, keepEffort, knownWorktrees, machineRows, placeMenuAt, makesBranch, projectAt, projectFolders, startPlace, tildePath } from "./agentpick.ts";
import { $, $$, type BackTo, bar, esc, ICON, invoke, PANE, S } from "../bar/core.ts";
import { completed, type FolderMenu, isOpen, menuKey, menuOf, NO_MENU, splitTyped, suggest } from "../addproject/foldercomplete.ts";
import { folderOn, type MachineFolder, type ProjectRow } from "../project/projects-state.ts";
import type { CachedViews } from "../center/cached.ts";
import type { HomeView } from "../center/home-state.ts";
import { composeField, composeMessage } from "../chat/compose.ts";
import { hints } from "../bar/hints.ts";
import type { Machine } from "../machines/machines.ts";

// ----- Start form -----

// A composer: the prompt, one pill for harness, model and thinking that
// opens the picker, and Machine and Folder menus under the box.

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
export function renderAgentNew(prompt = "", from: string | BackTo = "chat", at: AgentPlace | null = null) {
  const chat = S.view === "chat" ? S.session : null;
  S.view = "agentnew";
  AG.back = from;
  AG.form = { prompt, machine: at?.machine || lastMachine(at?.project), agent: null, cwd: at?.cwd || "", places: at?.places, chat, project: at?.project ?? null, checkout: DEFAULT_CHECKOUT, base: null, baseTyping: false, effort: null, open: null, query: "", typing: false };
  AG.error = "";
  AG.busy = false;
  drawFrame();
  $("#agprompt")?.focus();
  loadAgentHosts();
  invoke<AgentSession[]>("agent_sessions").then(list => { AG.sessions = list; if (S.view === "agentnew") drawParts(); }).catch(() => {});
  invoke<Machine[]>("list_machines").then(list => { KNOWN = list; if (S.view === "agentnew") drawParts(); }).catch(() => {});
  if (at?.project) loadBranches(at.project);
  loadProjects();
}

export function formHost() { return AG.hosts?.find(m => m.machine_id === AG.form.machine) ?? null; }
/// The agents the chosen machine offers; before one is picked, any machine's.
export function formAgents() {
  const host = formHost();
  const all = host ? host.agents : [...new Set((AG.hosts ?? []).flatMap(m => m.agents))];
  return all.sort(byPreference);
}

const CARET = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';
const UP = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M5 12l7-7 7 7"/></svg>';
const HOST = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/></svg>';
const FOLDER = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2z"/></svg>';
const PROJECT = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg>';
const BRANCH = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="7" r="2"/><path d="M6 7v10M18 9c0 5-6 4-11 8"/></svg>';

/// Redraws the form, keeping the prompt as typed when it shows already.
export function drawAgentNew() {
  if ($("#agbox")) drawParts();
  else drawFrame();
}

/// The form's frame: heading, prompt and the slots `drawParts` fills. The
/// prompt is drawn once, so redrawing the rest keeps what is typed.
function drawFrame() {
  const f = AG.form;
  const head = PANE ? "" : `<div class="dhead"><span class="tt"><b>Start a coding agent</b><small>It runs on your machine; follow it and answer it here</small></span>
      <button class="dbtn quiet" id="agList">All sessions</button></div>`;
  bar.innerHTML = `${head}<div class="agc-wrap" id="agwrap"><div class="agc">
      <h3 id="agtitle"></h3>
      <div class="agc-box" id="agbox">
        <textarea id="agprompt" rows="3" placeholder="Describe the task." aria-label="What should the agent do?">${esc(f.prompt)}</textarea>
        <div class="agc-row"><span class="sp"></span><span id="agpill"></span>
          <button class="agc-send" id="aggo" title="Start (⌘↩)" aria-label="Start"></button></div>
        <div id="agpick"></div>
      </div>
      <div class="agc-under" id="agunder"></div>
      <div class="agc-why" id="agwhy" hidden></div>
      <div id="agerr"></div>
      <div id="aghelp"></div>
    </div></div>${hints([[`⌘${ICON.ret}`, "start"], ["⌘M", "model"], ["esc", "cancel"]])}`;
  $$("#agprompt").addEventListener("input", e => {
    f.prompt = (e.target as HTMLTextAreaElement).value;
    if (AG.error === "prompt") { AG.error = ""; drawParts(); }
  });
  composeField($$<HTMLTextAreaElement>("#agprompt"));
  bar.onclick = onClick;
  drawParts();
}

/// Brings the form's choices in line with the machine picked, then draws
/// everything but the prompt.
function drawParts() {
  if (S.view !== "agentnew" || !$("#agbox")) return;
  const f = AG.form, hosts = AG.hosts ?? [];
  // A remembered machine that no longer runs agents is not picked.
  if (AG.hosts && f.machine && !formHost()) f.machine = "";
  f.agent = harnessFor(f.agent, formAgents(), AGENT_ORDER);
  f.effort = keepEffort(f.agent, f.effort);
  syncModel();
  const none = !!AG.hosts && !hosts.length;
  const missing = AG.error === "prompt";
  $$("#agbox").classList.toggle("err", missing);
  $$<HTMLTextAreaElement>("#agprompt").placeholder = missing ? "Say what the agent should do." : "Describe the task.";
  // The search and the folder field are drawn again; they keep the keys.
  const active = document.activeElement;
  const focused = active?.closest("#agpick") ? "#agpick .agc-search" : active?.id ? `#${active.id}` : "";
  const title = formTitle(f.project);
  $$("#agtitle").innerHTML = title.name ? `${esc(title.lead)} <b>${esc(title.name)}</b>?` : esc(title.lead);
  const go = $$<HTMLButtonElement>("#aggo");
  go.disabled = AG.busy || !hosts.length || !f.machine;
  go.title = f.machine ? "Start (⌘↩)" : "Pick a machine first";
  // The key hints give way to an open menu.
  bar.classList.toggle("agc-menuopen", !!f.open);
  go.innerHTML = AG.busy ? '<span class="dspin"></span>' : UP;
  drawPill(none);
  drawPicker();
  drawUnder();
  drawWhy();
  $$("#agerr").innerHTML = AG.error && !missing ? `<div class="derr">${esc(AG.error)}</div>` : "";
  $$("#aghelp").innerHTML = !AG.hosts ? '<div class="agc-note"><span class="dspin"></span> Looking for machines…</div>' : none ? `<div class="agc-help"><b>No machine can run an agent yet</b>
      <ol><li>Install beep-node on a computer that has Claude Code, Codex, Gemini CLI, pi or flsh.</li><li>Run <code>beep-node link</code> there.</li><li>Paste the link it prints into a chat here.</li></ol>
      <div><button class="dbtn" id="agmachines">Machines settings</button></div></div>` : "";
  if (focused === "#agpick .agc-search" || focused === "#agcwd" || focused === "#agbase") {
    const field = $<HTMLInputElement>(focused);
    field?.focus();
    field?.setSelectionRange(field.value.length, field.value.length);
  }
}

function drawPill(none: boolean) {
  const f = AG.form, el = $$("#agpill");
  if (none || !f.agent) {
    el.innerHTML = `<button class="agc-pill" disabled>No agent</button>`;
    return;
  }
  const inner = pillInner(f.agent, f.model ?? "", f.effort ?? null, f.modelFor ? formModelMachine() : null);
  el.innerHTML = `<button class="agc-pill" id="agpillb" aria-expanded="${f.open === "pick"}" title="Harness, model and thinking (⌘M)">${inner}</button>`;
}

/// The picker by the pill: harnesses, their models, their thinking.
function drawPicker() {
  const f = AG.form, el = $$("#agpick");
  if (f.open !== "pick" || !f.agent) { el.innerHTML = ""; closedPicker(el); return; }
  const host = formHost(), machine = f.modelFor ? formModelMachine() : null;
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

/// Project, Machine and Folder under the box, with whichever menu is open.
function drawUnder() {
  const f = AG.form, el = $$("#agunder"), host = formHost();
  const checkout = checkoutHere();
  const shown = folderPill(f.cwd, host?.workspace, host?.home);
  const folderText = shown.label ? `${esc(shown.label)} <small class="mono">${esc(shown.path)}</small>` : `<span class="mono">${esc(shown.path)}</span>`;
  const sub = checkout ? `<small>${esc(checkoutLabel(f.checkout ?? DEFAULT_CHECKOUT))}</small>` : "";
  const folder = f.typing
    ? `<span class="agc-type"><input id="agcwd" spellcheck="false" autocomplete="off" role="combobox" aria-label="Folder" placeholder="/path/to/folder or ~/folder" value="${esc(f.cwd)}"><span id="agcwdmenu"></span></span>`
    : `<button class="agc-pill" id="agfoldb" aria-expanded="${f.open === "folder"}" title="${esc(f.cwd.trim() || host?.workspace || "The machine's workspace")}">${FOLDER}${folderText}${sub}${CARET}</button>`;
  const base = checkout && makesBranch(f.checkout ?? DEFAULT_CHECKOUT) ? `<span class="bar"></span>${baseButton()}` : "";
  const project = `<button class="agc-pill" id="agprojb" aria-expanded="${f.open === "project"}" title="${f.project ? "The project this agent works on" : "Pick a project, or run in a folder"}">${PROJECT}${esc(f.project || "No project")}${CARET}</button>`;
  el.innerHTML = `${project}<span class="bar"></span><button class="agc-pill" id="aghostb" aria-expanded="${f.open === "host"}">${HOST}${esc(host ? host.machine_name : "Pick a machine")}${CARET}</button><span class="bar"></span>${folder}${base}${projectMenu()}${hostMenu()}${folderMenu(checkout)}${baseMenu()}`;
  placeMenu();
  if (f.typing) bindFolderField();
  if (f.baseTyping) bindBaseField();
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
function projectMenu() {
  const f = AG.form;
  if (f.open !== "project") return "";
  const machine = f.machine || null;
  const rows = (PROJECTS ?? []).map(p => {
    const here = machine ? p.places.find(x => x.machine === machine) : null;
    const where = here ? tildePath(here.cwd, formHost()?.home) : `${p.places.length} ${p.places.length === 1 ? "machine" : "machines"}`;
    return `<button data-agp="${esc(p.name)}" class="${f.project === p.name ? "on" : ""}"><span></span><span>${esc(p.name)}</span><small class="${here ? "mono" : ""}">${esc(where)}</small></button>`;
  });
  const note = PROJECTS === null ? ['<button disabled><span></span><span><span class="dspin"></span> Reading projects…</span><small></small></button>'] : [];
  const none = `<button data-agp="" class="${f.project ? "" : "on"}"><span></span>No project<small>run in a folder</small></button>`;
  return `<div class="agc-menu" role="menu">${[...note, ...rows, ...(rows.length || note.length ? ["<hr>"] : []), none].join("")}</div>`;
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

/// The Base button, which the new branch starts from: the ref picked, else the
/// project's default branch, else the checkout's HEAD.
function baseButton() {
  const f = AG.form, base = baseFor(f.base, formBranches());
  if (f.baseTyping) {
    return `<span class="agc-type base"><input id="agbase" spellcheck="false" autocomplete="off" aria-label="Base branch" placeholder="${esc(base || "HEAD")}" value="${esc(f.base ?? "")}"></span>`;
  }
  return `<button class="agc-pill" id="agbaseb" aria-expanded="${f.open === "base"}" title="The branch the new one starts from">${BRANCH}<small>from</small> <span class="mono">${esc(base || "HEAD")}</span>${CARET}</button>`;
}

/// The Base menu: the project's branches, the default first, and a row to
/// type any branch, tag or commit.
function baseMenu() {
  const f = AG.form;
  if (f.open !== "base") return "";
  const list = formBranches(), picked = baseFor(f.base, list);
  const rows = baseRows(list).map(name => `<button data-agb="${esc(name)}" class="${picked === name ? "on" : ""}"><span></span><span class="mono">${esc(name)}</span><small>${name === list?.default_branch ? "default" : ""}</small></button>`);
  const note = list === undefined ? ['<button disabled><span></span><span><span class="dspin"></span> Reading branches…</span><small></small></button>'] : [];
  return `<div class="agc-menu base" role="menu">${[...note, ...rows, ...(rows.length || note.length ? ["<hr>"] : []), `<button data-agbtype="1"><span></span>Type a branch or commit…<small></small></button>`].join("")}</div>`;
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

function hostMenu() {
  const f = AG.form;
  if (f.open !== "host") return "";
  const rows = machineRows(AG.hosts ?? [], KNOWN?.map(m => ({ id: m.id, name: m.name, online: m.online })) ?? null, a => agentKind(a).name, AGENT_ORDER);
  return `<div class="agc-menu cols" role="menu">${rows.map(m => `<button data-agm="${esc(m.id)}" class="${f.machine === m.id ? "on" : ""}"${m.enabled ? "" : " disabled"}><span class="dot${m.online ? "" : " off"}"></span><span>${esc(m.name)}</span><small>${esc(m.note)}</small></button>`).join("")}</div>`;
}

/// Whether the folder is one of the project's checkouts on the machine
/// picked (any of them before one is).
function checkoutHere() {
  const f = AG.form, cwd = f.cwd.trim();
  if (!cwd || !f.places?.length) return false;
  const machine = formModelMachine();
  return f.places.some(p => (!machine || p.machine === machine) && (p.cwd === cwd || folderOn(p.cwd, null, null) === cwd));
}

function folderMenu(checkout: boolean) {
  const f = AG.form;
  if (f.open !== "folder") return "";
  const machine = f.machine || null, host = formHost();
  if (!f.project) {
    // Outside a project: the workspace, then the projects checked out on
    // the machine; picking one sets the Project pill.
    const rows = folderRows(f.cwd, host?.workspace, host?.home, projectFolders(PROJECTS ?? [], machine, host?.home)).map(r => {
      if (r.kind === "rule") return "<hr>";
      if (r.kind === "type") return `<button data-agtype="1"><span></span>Type a folder…<small></small></button>`;
      if (r.kind === "workspace") return `<button data-agf="" class="${r.on ? "on" : ""}"><span></span>Workspace<small class="mono">${esc(r.shown)}</small></button>`;
      return `<button data-agf="${esc(r.cwd)}" class="${r.on ? "on" : ""}"><span></span><span>${esc(r.project)}</span><small class="mono">${esc(r.shown)}</small></button>`;
    });
    return `<div class="agc-menu folder" role="menu">${rows.join("")}</div>`;
  }
  const places = [...new Set((f.places ?? []).filter(p => !machine || p.machine === machine).map(p => machine ? p.cwd : folderOn(p.cwd, null, null)))];
  const pick = f.checkout ?? DEFAULT_CHECKOUT;
  const same = (p: CheckoutPick) => JSON.stringify(p) === JSON.stringify(pick);
  const trees = checkout ? knownWorktrees(AG.sessions, formModelMachine(), f.cwd) : [];
  const workspace = host?.workspace ? tildePath(host.workspace, host.home) : "~";
  const rows = [
    ...places.map(p => `<button data-agf="${esc(p)}" class="${f.cwd.trim() === p ? "on" : ""}"><span></span><span class="mono">${esc(tildePath(p, host?.home))}</span><small>checkout</small></button>`),
    ...(checkout ? ["<hr>",
      `<button data-agk="current" class="${same("current") ? "on" : ""}"><span></span>Current checkout<small>as it is</small></button>`,
      `<button data-agk="branch" class="${same("branch") ? "on" : ""}"><span></span>New branch<small>in the checkout, named from the prompt</small></button>`,
      `<button data-agk="new" class="${same("new") ? "on" : ""}"><span></span>New worktree<small>new branch in .worktrees/</small></button>`,
      ...trees.map(t => `<button data-agw="${esc(t)}" class="${same({ worktree: t }) ? "on" : ""}"><span></span><span class="mono">${esc(t.split("/").slice(-2).join("/"))}</span><small>worktree</small></button>`)] : []),
    ...(places.length || checkout ? ["<hr>"] : []),
    `<button data-agf="" class="${f.cwd.trim() ? "" : "on"}"><span></span>Workspace<small class="mono">${esc(workspace)}</small></button>`,
    `<button data-agtype="1"><span></span>Type a folder…<small></small></button>`,
  ];
  return `<div class="agc-menu folder" role="menu">${rows.join("")}</div>`;
}

/// The line under the box until a machine is picked.
function drawWhy() {
  const f = AG.form, el = $$("#agwhy");
  const show = !f.machine && !!AG.hosts?.length;
  el.hidden = !show;
  el.innerHTML = show ? '<span class="ddot off"></span><span>Pick a machine to start.</span>' : "";
}

function onClick(e: MouseEvent) {
  if (S.view !== "agentnew") return;
  const f = AG.form, target = e.target as HTMLElement;
  const at = (sel: string) => target.closest<HTMLElement>(sel);
  if (at("#aggo")) return startAgent();
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
    const input = $<HTMLInputElement>("#agcwd");
    input?.focus();
    input?.setSelectionRange(input.value.length, input.value.length);
    return;
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
  const input = $<HTMLInputElement>("#agbase");
  input?.focus();
  input?.setSelectionRange(input.value.length, input.value.length);
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
  const ask = ++lookup, machine = formModelMachine(), typed = splitTyped(input.value);
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
  const box = $("#agcwdmenu");
  if (!box) return;
  input.setAttribute("aria-expanded", String(isOpen(menu)));
  if (!isOpen(menu)) { box.innerHTML = ""; return; }
  const { dir, index } = menu;
  box.innerHTML = `<div class="agc-menu" role="listbox">${menu.items.map((name, i) => `<button data-agfi="${i}" class="${i === index ? "on" : ""}" role="option"><span></span><span class="mono">${esc(name)}</span><small></small></button>`).join("")}</div>`;
  box.querySelectorAll<HTMLElement>("[data-agfi]").forEach(b => {
    b.addEventListener("mousedown", e => e.preventDefault());
    b.addEventListener("click", e => {
      e.stopPropagation();
      AG.form.cwd = input.value = completed(dir, menu.items[Number(b.dataset.agfi)]);
      lookFolders(input);
    });
  });
}

// ----- Models -----

/// The machine the picker lists models on: the one picked, or null.
function formModelMachine() {
  return AG.form.machine || null;
}

/// Keeps the model with the machine and harness it was picked for, and
/// asks for their models when not known lately.
function syncModel() {
  const f = AG.form, machine = formModelMachine();
  const key = machine && f.agent ? `${machine}/${f.agent}` : "";
  if (f.modelFor !== key) { f.model = ""; f.modelFor = key; }
  if (!key) return;
  modelsOn(machine!, f.agent!, () => { if (S.view === "agentnew" && AG.form.modelFor === key) drawPill(false); });
}

// ----- Start -----

export async function startAgent() {
  const f = AG.form;
  if (AG.busy || S.view !== "agentnew") return;
  if (!f.prompt.trim()) { AG.error = "prompt"; drawParts(); return $("#agprompt")?.focus(); }
  if (!f.machine) { f.open = "host"; return drawParts(); }
  AG.busy = true;
  AG.error = "";
  f.open = null;
  drawParts();
  try {
    const model = f.model?.trim() && f.modelFor ? f.model.trim() : null;
    const machine = f.machine;
    keepMachine(f.project, machine);
    // In a project's checkout, an existing worktree is a folder to start
    // in, and the node makes a new branch or worktree before it starts
    // only when one is picked.
    const { cwd, checkout } = checkoutHere()
      ? startPlace(f.checkout ?? DEFAULT_CHECKOUT, f.cwd.trim(), baseFor(f.base, formBranches()))
      : { cwd: f.cwd.trim(), checkout: null };
    const started = await invoke<Started>("agent_start", {
      machineId: machine,
      agent: f.agent,
      cwd: cwd || null,
      prompt: composeMessage(f.prompt),
      chat: f.chat,
      model,
      effort: f.effort || null,
      checkout,
    });
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
