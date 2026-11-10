import { createElement } from "react";
import { removeWindowPart, removeWindowParts, showWindowParts } from "../center/window-root.ts";
import { ProjectSessions } from "./sessions-view.tsx";
import { refreshMoves, MV } from "../agents/move-store.ts";
import { subscribeChanges, subscribeEach, topic } from "../ui/mesh-events.ts";
import { AMBIENT_MS, SLOW_MS } from "../ui/active.ts";
// Reading a project page: its view, agent, machines and sessions, kept
// fresh while the user moves between projects, and prefetched ahead.
import { sessionVisible } from "../agents/closing.ts";
import { failure, formatAgo, reason } from "../ui/format.ts";
import { copy } from "../ui/parts.ts";
import { type MenuItem } from "../ui/menu.ts";
import { Changes, UNWATCHED_MS } from "../ui/pagecache.ts";
import { keyedReads, parseRecent, prefetchOrder, touchRecent } from "./projectloads.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { ElsewhereSection, FoundSection } from "./overview-view.tsx";
import type { RecentSessions } from "./split-view.tsx";
import { foundFor, withSyncthing, projectPastSessions, nextSessions, type Project, type ProjectMachineRow, type SessionRow, type SyncthingPlace, type WhereRow } from "./projects-state.ts";
import { repoFromRemote } from "../addproject/importproject-state.ts";
import { el as h } from "../ui/dom.ts";
import { ITEMS_SHOWN, repoUrl, reviewStart } from "../center/openitems-state.ts";
import { outsideThreads } from "../review/branchwork.ts";
import { agentFor, pageAgent, type ProjectAgent, type ProjectView } from "./agent.ts";
import { state, pages, keep, pageFresh } from "../center/state.ts";
import { openUrl, refreshGit, replyItem } from "../center/openitems.ts";
import { currentLocation, go } from "../center/views.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { projectRows, importSheet, addProject, importFromPage, loadFound } from "../center/sidebar.ts";
import { loadOverview } from "./projects.ts";
import { terms, fsync, revs, thr, runs, projectsEl, redrawProjects, redrawProjectsPage } from "./sections.ts";
import { drawProjects } from "./page.ts";
import { refresh, onScreen } from "../center/refresh.ts";
import { openAddToMachine } from "./addmachine.ts";
import type { RunningAgent } from "../agents/running-state.ts";
import { syncRepairPrompt, agentCanFix, gitLine, movingLabel, moveKey, moveTitle, withRow, type GitMove, type GitRow } from "./gitsync-state.ts";
import { openHandoff } from "./handoff.tsx";

/// Each project's tasks, workers and chat, read one at a time per project
/// and kept under the project they were asked for, so an answer for the
/// project left behind never draws over the one opened since.
const viewReads = keyedReads<ProjectView>({
  read: name => {
    const agent = agentFor(state.agents, name);
    if (!agent) return Promise.reject(new Error(`${name} has no project agent`));
    return invoke<ProjectView>("center_project", { projectId: agent.project_id, chatId: agent.chat_id });
  },
  got: (name, view) => {
    state.pview.set(name, { view, error: null });
    keep("view", name, view);
  },
  failed: (name, e) => state.pview.set(name, { view: state.pview.get(name)?.view ?? null, error: failure(e) }),
  shown: () => (currentLocation()?.view === "projects" ? state.picked : null),
  redraw: redrawProjectsPage,
});

/// Reads project `name`'s view when it has an agent; `after` asks for a
/// read that starts after now, as once something changed.
function loadView(name: string, after = false) {
  return agentFor(state.agents, name) ? viewReads.load(name, after) : undefined;
}

/// Reads the project agents and, beside them, the picked project's tasks,
/// workers and chat.
let agentLoading = false, agentAgain = false;
export async function loadProjectAgent(after = true) {
  if (agentLoading) { agentAgain = true; return; }
  agentLoading = true;
  const picked = state.picked;
  const early = picked ? loadView(picked, after) : undefined;

  // The project agent needs a subscription: no panel, and no call to ask for one.
  if (!state.subscribed) {
    agentLoading = false;
    state.agents = null;
    if (state.picked && (state.picked !== picked || !early)) loadView(state.picked);
    redrawProjectsPage();
    return;
  }

  try {
    state.agents = await invoke<ProjectAgent[] | null>("center_projects");
  } catch (e) {
    console.warn("project agents:", e);
    if (state.agents === undefined) state.agents = null;
  } finally {
    agentLoading = false;
  }

  // A project whose agent was not known before reads now.
  if (state.picked && (state.picked !== picked || !early)) loadView(state.picked);
  // The header's Board and List switch shows once the project has an agent.
  redrawProjectsPage();
  if (agentAgain) { agentAgain = false; loadProjectAgent(); }
}

// ---------- Switching projects ----------

/// The projects opened last, newest first, kept for next time.
const RECENT_KEY = "beep.center.recentProjects";
let recent: string[] = [];
try { recent = parseRecent(localStorage.getItem(RECENT_KEY)); } catch {}

export function noteRecent(name: string) {
  recent = touchRecent(recent, name);
  try { localStorage.setItem(RECENT_KEY, JSON.stringify(recent)); } catch {}
}

// What a project page shows changes with these topics, which the window
// follows on every page. A section reads again on opening only after one
// of its topics changed since its last read; one no topic follows reads
// once it is `UNWATCHED_MS` old. Sync details follow their own changes.
const whereChanges = new Changes();
const sessionChanges = new Changes();
const termChanges = new Changes();
const overviewChanges = new Changes();
subscribeEach([topic("projects"), topic("machines"), topic("sync")], () => whereChanges.changed());
// Home changes with every session's state and with moves.
subscribeEach(topic("home"), () => sessionChanges.changed());
for (const name of ["sessions-changed", "agent-title-changed", "reconnected"]) listen(name, () => sessionChanges.changed());
subscribeEach(topic("terminals"), () => termChanges.changed());
subscribeEach(topic("projects"), () => overviewChanges.changed());

/// Marks what opening project `name` reads behind its kept page.
export function markDue(name: string) {
  if (whereChanges.dirty(name)) whereDue.add(name);
  if (sessionChanges.dirty(name)) sessionsDue.add(name);
  if (pages.due("gitsync", name, UNWATCHED_MS)) gitDue.add(name);
}

/// Reads project `name`'s terminals once they changed since the last read.
function loadTerms(name: string) {
  if (!termChanges.dirty(name)) return;
  termChanges.read(name);
  return terms.load(name);
}

/// On opening project `name`, behind what the page drew from the cache:
/// what changed since it was read, and what no topic follows once it is
/// old. The drawing asks for the rest as it is due.
export function revalidate(name: string) {
  if (pages.due("view", name, UNWATCHED_MS)) loadView(name);
  const agent = pageAgent(state.agents, name, state.subscribed);

  if (agent) {
    thr.ensure(agent.project_id);
    runs.ensure(agent.project_id);
  }

  loadTerms(name);
  fsync.ensure(name, UNWATCHED_MS);
  if (sessionsDue.has(name) || !state.sessions.has(name)) loadSessions(name);
}

/// The Projects overview: the found checkouts and the task summaries, read
/// the first time and after a project change.
export function revalidateOverview() {
  if (!state.found || overviewChanges.dirty("found")) { overviewChanges.read("found"); void loadFound(); }
  if (state.overview === undefined || overviewChanges.dirty("overview")) { overviewChanges.read("overview"); void loadOverview(); }
}

/// Reads project `name` ahead without drawing it: only what changed since
/// its last read and is old as well, so pointing at projects asks less
/// often than opening them would.
function prefetch(name: string): Promise<unknown> {
  const p = (projectRows() || []).find(r => r.name === name);
  if (!p?.imported) return Promise.resolve();
  const agent = pageAgent(state.agents, name, state.subscribed);
  const reads: (Promise<unknown> | undefined)[] = [revs.ensure(name), pages.due("terminals", name) ? loadTerms(name) : undefined, fsync.ensure(name, UNWATCHED_MS)];

  if (agent) {
    if (agentFor(state.agents, name) && pages.due("view", name, UNWATCHED_MS)) reads.push(loadView(name));
    // The code host's lists at their own pace: an ask keeps the server
    // reading the host for that project a while.
    reads.push(thr.ensure(agent.project_id), runs.ensure(agent.project_id));
  }

  if (whereChanges.dirty(name) && pages.due("where", name)) reads.push(loadWhere(name));
  if (sessionChanges.dirty(name) && pages.due("sessions", name)) reads.push(loadSessions(name));
  return Promise.allSettled(reads);
}

/// How long the pointer rests on a project before it is read ahead.
const HOVER_MS = 150;
/// How long after a project opens, the window idle, the projects opened
/// last are read ahead.
const IDLE_PREFETCH_MS = 4000;

/// Reads project `name` ahead once the pointer rests on `el`.
export function prefetchOnHover(el: HTMLElement, name: string) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  el.addEventListener("mouseenter", () => { timer = setTimeout(() => prefetch(name), HOVER_MS); });
  el.addEventListener("mouseleave", () => clearTimeout(timer));
}

/// Reads the projects opened last ahead, one after another, while the
/// window shows; one round at a time.
let prefetching = false, idleTimer: ReturnType<typeof setTimeout> | undefined;
export function prefetchRecentSoon() {
  clearTimeout(idleTimer);
  idleTimer = setTimeout(async () => {
    if (prefetching || !onScreen()) return;
    prefetching = true;

    try {
      const names = new Set((projectRows() || []).map(r => r.name));
      for (const name of prefetchOrder(recent, state.picked, n => names.has(n))) await prefetch(name);
    } finally {
      prefetching = false;
    }
  }, IDLE_PREFETCH_MS);
}

/// A project's checkout on each machine, for the start form.
export function projectPlaces(p: Project) {
  return p.machines.filter(m => m.folder).map(m => ({ machine: m.id, cwd: m.folder }));
}

/// What became of "Copy here", by project and machine.
const copies = new Map<string, { state: "copying" | "already" | "failed"; text?: string }>();

/// Copies `p` onto machine `m`; a server too old to clones instead.
async function copyHere(p: Project, m: WhereRow, key: string) {
  copies.set(key, { state: "copying" });
  redrawProjectsPage();

  try {
    const copy = await invoke<{ folder: string; from: string | null } | null>("project_copy", { project: p.name, machineId: m.id });
    copies.delete(key);
    if (!copy) importSheet(repoFromRemote(p.url), m.id);
    else if (!copy.from) copies.set(key, { state: "already" });
    refresh();
    await loadWhere(p.name);
  } catch (e) {
    copies.set(key, { state: "failed", text: reason(e) });
  }

  redrawProjectsPage();
}

/// The projects whose checkouts' branches are being read, and those to read
/// again on the next draw.
const readingGit = new Set<string>();
export const gitDue = new Set<string>();

/// Reads each checkout's branch and distance from its upstream. Kept apart
/// from "where" so a slow fetch never holds the page.
export async function loadGit(name: string) {
  if (readingGit.has(name)) return;
  readingGit.add(name);
  gitDue.delete(name);

  try {
    const rows = await invoke<GitRow[]>("project_git_state", { project: name });
    state.gitsync.set(name, rows);
    keep("gitsync", name, rows);
  } catch (e) {
    console.warn("project git state:", e);
    if (!state.gitsync.has(name)) state.gitsync.set(name, []);
  } finally {
    readingGit.delete(name);
  }

  if (currentLocation()?.view === "projects" && state.picked === name) redrawProjectsPage();
}

/// A running move's direction and a refused one's reason, by project and machine.
const moves = new Map<string, { moving: GitMove | null; failed?: string }>();

/// Pulls or pushes on one machine; the counts come back with the answer.
async function gitMove(p: Project, m: WhereRow, row: GitRow, move: GitMove) {
  const key = moveKey(p.name, m.id);
  if (moves.get(key)?.moving || !row.branch) return;
  moves.set(key, { moving: move });
  redrawProjectsPage();

  try {
    const next = await invoke<GitRow>("project_git_move", { project: p.name, machineId: m.id, branch: row.branch, direction: move });
    moves.delete(key);
    const rows = withRow(state.gitsync.get(p.name), next);
    state.gitsync.set(p.name, rows);
    keep("gitsync", p.name, rows);
  } catch (e) {
    moves.set(key, { moving: null, failed: reason(e) });
  }

  redrawProjectsPage();
}

export function gitControls(p: Project, m: WhereRow, online: boolean) {
  const row = state.gitsync.get(p.name)?.find(r => r.machine_id === m.id);
  if (!row) return null;
  const line = gitLine(row), move = moves.get(moveKey(p.name, m.id)), failed = move?.failed ?? null;
  const buttons = line.buttons ? (["pull", "push"] as const).map(direction => {
    const n = direction === "pull" ? row.behind : row.ahead, active = direction === "pull" ? line.pull : line.push;
    return { direction, count: n, moving: move?.moving === direction, title: move?.moving === direction ? movingLabel(direction) : moveTitle(direction, n), label: moveTitle(direction, n), disabled: !online || !!move?.moving || !active, run: () => void gitMove(p, m, row, direction) };
  }) : [];
  return { line, buttons, failed: line.buttons ? failed : null,
    handoff: line.buttons && p.machines.length > 1 && !m.syncthing ? () => openHandoff(p, m.id, () => { loadGit(p.name); refresh(); }) : null,
    fix: line.buttons && failed && agentCanFix(failed) ? () => void openSyncRepair(p.name, m.id, failed) : null };
}

export function missingMachine(p: Project, m: WhereRow) {
  const key = `${p.name}\n${m.id}`, copied = copies.get(key);
  const text = copied?.text || m.text || "", title = m.kind === "none" && m.checked_at ? `Checked ${formatAgo(m.checked_at, "since")}` : "";
  let action: { label: string; disabled: boolean; run(): void } | null = null;
  if (!state.home?.machines?.some(x => x.id === m.id && x.can_host === false)) {
    if (m.kind === "none" && m.add) action = { label: "Add", disabled: false, run: () => openAddToMachine({ project: p.name, machine: m, os: state.home?.machines?.find(x => x.id === m.id)?.env?.os, macId: state.mac?.id || null, added: () => { refresh(); loadWhere(p.name); } }) };
    else if (m.kind === "none" && m.copy && copied?.state !== "failed") action = { label: copied ? "Copying…" : "Copy here", disabled: !!copied, run: () => void copyHere(p, m, key) };
    else if (m.kind === "none" && p.url) { const url = p.url; action = { label: "Clone here", disabled: false, run: () => importSheet(repoFromRemote(url), m.id) }; }
  }
  return { text, title, syncthing: m.kind === "none" ? m.syncthing ?? null : null, action };
}

/// Opens a reviewable draft; no agent starts until the user sends it.
export async function openSyncRepair(project: string, machine: string, error: string) {
  const p = projectRows()?.find(p => p.name === project);
  const place = p?.machines.find(m => m.id === machine);
  if (!p || !place) return;
  let sessions: SessionRow[] = [];

  try { sessions = await invoke<SessionRow[]>("project_sessions", { project }); }
  catch (e) { console.warn("repair sessions:", e); }

  const prompt = syncRepairPrompt({ project, machine, error, checkouts: p.machines, git: state.gitsync.get(project) ?? [], sessions });
  go("pane", { agentnew: { prompt, at: { machine, cwd: place.folder, places: projectPlaces(p), project } } });
}

/// Actions shared by a checkout's machine header and table row.
export function checkoutItems(p: Project, m: WhereRow, path: string): MenuItem[] {
  return [
    { label: "Start an agent here", on: () => go("pane", { agentnew: { prompt: "", at: { machine: m.id, cwd: path, places: projectPlaces(p), project: p.name } } }) },
    ...(m.mac ? [{ label: "Open in Finder", on: () => void invoke("open_folder", { path }).catch(e => console.warn("open folder:", e)) }] : []),
    { label: "Copy path", on: () => void copy(path) },
  ];
}

/// Follows changes to the selected project's checkouts on each machine.
export const readingWhere = new Set<string>();
subscribeChanges([topic("projects"), topic("machines"), topic("sync")], { deliver: "latest", receive: async () => {
  const name = state.picked;
  if (name) { await whereReads.get(name); await loadWhere(name); }
} }, { when: () => currentLocation()?.view === "projects" && !!state.picked, wait: AMBIENT_MS });
/// Projects whose kept places are due another read on the next draw.
export const whereDue = new Set<string>();
const whereReads = new Map<string, Promise<void>>();
export function loadWhere(name: string): Promise<void> {
  const pending = whereReads.get(name);
  if (pending) return pending;
  const read = readWhere(name).finally(() => whereReads.delete(name));
  whereReads.set(name, read);
  return read;
}
async function readWhere(name: string) {
  if (readingWhere.has(name)) return;
  readingWhere.add(name);
  whereDue.delete(name);
  whereChanges.read(name);
  const before = state.where.get(name);
  // A kept list shows a spinner in its header meanwhile.
  if (before && state.picked === name) queueMicrotask(redrawProjects);
  let got: ProjectMachineRow[] | null = null;

  try {
    const [machines, places] = await Promise.all([
      invoke<ProjectMachineRow[] | null>("project_machines", { project: name }),
      invoke<SyncthingPlace[]>("project_syncthing", { project: name }).catch(e => { console.warn("project syncthing:", e); return []; }),
    ]);
    got = withSyncthing(machines, places);
  } catch (e) {
    console.warn("project machines:", e);
    whereChanges.changed();
  } finally {
    readingWhere.delete(name);
  }

  // A failed read keeps the last list it had.
  if (got || !before) state.where.set(name, got);
  if (got && !got.some(m => m.checking)) keep("where", name, got);
  const folders = (list: ProjectMachineRow[] | null) => (list || []).filter(m => m.folder).map(m => `${m.id} ${m.folder}`).join("\n");

  if (got && before !== undefined && folders(before) !== folders(got)) {
    sessionsDue.add(name);
    refresh();
  }

  // The header's Open in follows this Mac's checkout.
  if (state.picked === name) redrawProjectsPage();
}

/// Ended sessions in the project's folders, newest first; each opens.
const sessionLists = new Map<boolean, { project: string; host: HTMLElement }>();
const sessionPart = (all: boolean) => `project-sessions:${all ? "all" : "recent"}` as const;

export function disposeSessionPanel() { removeWindowParts([...sessionLists.keys()].map(sessionPart)); sessionLists.clear(); }

function sessionListHost(project: string, rows: SessionRow[], all: boolean) {
  let current = sessionLists.get(all);
  if (current && current.project !== project) { removeWindowPart(sessionPart(all)); sessionLists.delete(all); current = undefined; }
  if (!current) {
    const host = h("div", "cc-panel");
    current = { project, host };
    sessionLists.set(all, current);
  }
  const list = current;
  showWindowParts([{ part: sessionPart(all), host: list.host, identity: project, content: createElement(ProjectSessions, {
    rows, all, open: openAgentRow,
    menu: (event: MouseEvent, row: SessionRow) => void sessionMenu(event, row, selected => go("pane", { agent: selected })),
  }) }]);
  return list.host;
}

/// The project's past sessions, newest first: five rows tall unless `all`,
/// with the note that they are being read again.
export function sessionsList(p: Project, running: RunningAgent[], all: boolean): RecentSessions {
  const sessions = state.sessions.get(p.name);
  const past = projectPastSessions(sessions, running, [...MV.records.values()]).filter(sessionVisible);
  if ((!p.imported || sessions === undefined || !past.length)) disposeSessionPanel();
  if ((sessions === undefined || sessionsDue.has(p.name)) && p.imported) loadSessions(p.name);
  const freshness = sessions !== undefined ? pageFresh("sessions", p.name, readingSessions.has(p.name)) : null;

  if (!p.imported) return { host: null, empty: "Import this project to see its sessions.", count: past.length, freshness };
  if (sessions === undefined) return { host: null, empty: "Reading sessions…", count: past.length, freshness };
  if (!past.length) return { host: null, empty: "No past sessions yet.", count: past.length, freshness };
  return { host: sessionListHost(p.name, past, all), empty: null, count: past.length, freshness };
}

/// The agent view for a session row.
function openAgentRow(r: SessionRow) { go("pane", { agent: { kind: "agent", ...r } }); }

/// The projects whose sessions are being read, and those to read again on the
/// next draw.
const readingSessions = new Set<string>();
export const sessionsDue = new Set<string>();
const sessionReads = new Map<string, Promise<void>>();
export async function loadSessions(name: string): Promise<void> {
  const pending = sessionReads.get(name);
  if (pending) { await pending; if (sessionsDue.has(name)) return loadSessions(name); return; }
  const read = readSessions(name).finally(() => sessionReads.delete(name));
  sessionReads.set(name, read);
  return read;
}
async function readSessions(name: string) {
  readingSessions.add(name);
  sessionsDue.delete(name);
  sessionChanges.read(name);
  let got: SessionRow[] | null = null;

  try {
    [got] = await Promise.all([invoke<SessionRow[]>("project_sessions", { project: name }), refreshMoves()]);
  } catch (e) {
    console.warn("project sessions:", e);
    sessionChanges.changed();
  } finally {
    readingSessions.delete(name);
  }

  const { list } = nextSessions(state.sessions.get(name), got);
  state.sessions.set(name, list);
  if (got) keep("sessions", name, list);
  if (currentLocation()?.view !== "projects" || state.picked !== name) return;
  // The draw also drops the Updating note.
  drawProjects(projectsEl());
}

/// The checkouts the machines found that are not projects yet: the first
/// three, each with Import; the sheet lists them all.
let foundHost: HTMLElement | null = null;
export function disposeFoundSection() {
  if (!foundHost) return;
  removeWindowPart("project-found");
  foundHost = null;
}
export function foundSection(enabled: boolean) {
  if (!enabled || !state.found?.length) { disposeFoundSection(); return null; }
  if (!foundHost) { foundHost = h("div"); foundHost.id = "sec-found"; }
  showWindowParts([{ part: "project-found", host: foundHost, content: createElement(FoundSection, {
    rows: state.found, importing: state.importing, error: state.foundError, importOne: importFromPage, add: addProject,
  }) }]);
  return foundHost;
}

let askedGit = false;
let elsewhereHost: HTMLElement | null = null;
export function disposeElsewhereSection() {
  if (!elsewhereHost) return;
  removeWindowPart("project-elsewhere");
  elsewhereHost = null;
}
/// The open PRs and issues in repos without a project, each repo with
/// Import project, under the Projects overview.
export function elsewhereSection(rows: Project[], enabled: boolean) {
  if (!enabled) { disposeElsewhereSection(); return null; }
  if (!state.git) {
    if (!askedGit) { askedGit = true; refreshGit().then(redrawProjects); }
    disposeElsewhereSection();
    return null;
  }

  const repos = outsideThreads(state.git, rows).slice(0, 8);
  if (!repos.length) { disposeElsewhereSection(); return null; }
  const body = repos.map((r, i) => {
    // A checkout the machines found imports as it is; otherwise the sheet clones.
    const here = foundFor(state.found, r.repo);
    const failed = here && state.foundError?.name === here.name ? state.foundError.message : null;
    const sub = failed ?? (here ? `on ${here.places.map(p => p.machine_name).join(", ")}` : r.host);
    return { key: `${r.host} ${r.repo}`, name: r.repo, sub, url: repoUrl(r.items[0].url), last: r.last,
      importing: !!here && state.importing === here.name, disabled: !!here && state.importing != null, primary: i === 0,
      items: r.items.slice(0, ITEMS_SHOWN), import: () => { if (here) void importFromPage(here); else importSheet(r.repo); } };
  });
  if (!elsewhereHost) { elsewhereHost = h("div"); elsewhereHost.id = "sec-elsewhere"; }
  showWindowParts([{ part: "project-elsewhere", host: elsewhereHost, content: createElement(ElsewhereSection, { repos: body, open: openUrl,
    review: item => go("pane", { agentnew: reviewStart(item, null, state.home?.machines?.filter(m => m.can_host !== false)) }),
    reply: item => { void replyItem(item); },
  }) }]);
  return elsewhereHost;
}

// Reopened sessions and moves change independently of project membership.
const projectSessionsShown = () => currentLocation()?.view === "projects" && !!state.picked;
async function refreshProjectSessions() {
  const name = state.picked;
  if (name) { sessionsDue.add(name); await loadSessions(name); }
}
// Agents report many times a second during turns; the list follows every two seconds.
subscribeChanges([topic("agents"), topic("moves")], { deliver: "latest", receive: refreshProjectSessions }, { when: projectSessionsShown, wait: SLOW_MS });
listen("agent-title-changed", () => {
  if (projectSessionsShown()) void refreshProjectSessions().catch(e => console.warn("project session titles:", e));
});
