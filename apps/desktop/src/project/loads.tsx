import { removeWindowPart, showWindowParts } from "../center/window-root.tsx";
import { ProjectSessions } from "./sessions-view.tsx";
import { retainedHost } from "./page-view.tsx";
import { refreshMoves, moveStore } from "../agents/move-store.ts";
import { subscribeChanges, subscribeEach, topic } from "../ui/mesh-events.ts";
import { AMBIENT_MS, SLOW_MS } from "../ui/active.ts";
// Reading a project page: its machines, lists and sessions, kept fresh
// while the user moves between projects, and prefetched ahead.
import { sessionVisible } from "../agents/closing.ts";
import { formatAgo, reason } from "../ui/format.ts";
import { copy } from "../ui/parts.tsx";
import { type MenuItem } from "../ui/menu.tsx";
import { Changes, UNWATCHED_MS } from "../ui/pagecache.ts";
import { parseRecent, prefetchOrder, touchRecent } from "./projectloads.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { FoundSection, RepoSection } from "./outside-lists.tsx";
import { ProjectRail, type RecentSessions } from "./split-view.tsx";
import { shell } from "../center/window-shell.tsx";
import { foundFor, withSyncthing, projectName, projectRunning, projectPastSessions, nextSessions, type Project, type ProjectMachineRow, type SessionRow, type SyncthingPlace, type WhereRow } from "./projects-state.ts";
import { repoFromRemote } from "../addproject/importproject-state.ts";
import { outsideThreads } from "../review/branchwork.ts";
import { localProjectId } from "./localid.ts";
import { state, pages, keep, pageFresh } from "../center/state.ts";
import { openUrl, refreshGit } from "../center/openitems.ts";
import { currentLocation, go } from "../center/views.tsx";
import { projectRows, importSheet, addProject, importFromPage, loadFound, drawRail, openProjects } from "../center/sidebar.tsx";
import { terms, fsync, thr, runs, redrawProjects, redrawProjectsPage, repairButton } from "./sections.ts";
import { drawProjects } from "./page.tsx";
import { refresh, onScreen } from "../center/refresh.ts";
import { openAddToMachine } from "./addmachine.tsx";
import type { RunningAgent } from "../agents/running-state.ts";
import { agentCanFix, gitLine, movingLabel, moveKey, moveTitle, withRow, type GitMove, type GitRow } from "./gitsync-state.ts";
import { openHandoff } from "./handoff.tsx";
import { perfFresh } from "../ui/perf.ts";

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
const foundChanges = new Changes();
subscribeEach([topic("projects"), topic("machines"), topic("sync")], () => whereChanges.changed());
// Home changes with every session's state and with moves.
subscribeEach(topic("home"), () => sessionChanges.changed());
for (const name of ["sessions-changed", "agent-title-changed", "reconnected"]) listen(name, () => sessionChanges.changed());
subscribeEach(topic("terminals"), () => termChanges.changed());
subscribeEach(topic("projects"), () => foundChanges.changed());

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
  // The page drew its kept sessions: time until they are read again.
  const kept = pages.at("sessions", name) !== undefined;
  const reads: (Promise<unknown> | undefined)[] = [];
  const id = localProjectId(name);
  thr.ensure(id);
  runs.ensure(id);

  loadTerms(name);
  fsync.ensure(name, UNWATCHED_MS);
  if (sessionsDue.has(name) || !state.sessions.has(name)) reads.push(loadSessions(name));
  const fresh = reads.filter(read => read !== undefined);
  if (kept && fresh.length) void Promise.allSettled(fresh).then(perfFresh("project"));
}

/// The Projects overview: the found checkouts, read the first time and after
/// a project change.
export function revalidateOverview() {
  if (!state.found || foundChanges.dirty("found")) { foundChanges.read("found"); void loadFound(); }
}

/// Reads project `name` ahead without drawing it: only what changed since
/// its last read and is old as well, so pointing at projects asks less
/// often than opening them would.
function prefetch(name: string): Promise<unknown> {
  const p = (projectRows() || []).find(r => r.name === name);
  if (!p?.imported) return Promise.resolve();
  const id = localProjectId(name);
  // The code host's lists at their own pace.
  const reads: (Promise<unknown> | undefined)[] = [pages.due("terminals", name) ? loadTerms(name) : undefined, fsync.ensure(name, UNWATCHED_MS), thr.ensure(id), runs.ensure(id)];

  if (whereChanges.dirty(name) && pages.due("where", name)) reads.push(loadWhere(name));
  if (sessionChanges.dirty(name) && pages.due("sessions", name)) reads.push(loadSessions(name));
  return Promise.allSettled(reads);
}

/// How long the pointer rests on a project before it is read ahead.
const HOVER_MS = 150;
/// How long after a project opens, the window idle, the projects opened
/// last are read ahead.
const IDLE_PREFETCH_MS = 4000;

/// Reads project `name` ahead once the pointer rests on the element given these handlers.
/// The pointer rests on one element at a time, so one timer serves them all.
let hoverTimer: ReturnType<typeof setTimeout> | undefined;
export function prefetchOnHover(name: string) {
  return {
    onMouseEnter: () => { clearTimeout(hoverTimer); hoverTimer = setTimeout(() => prefetch(name), HOVER_MS); },
    onMouseLeave: () => clearTimeout(hoverTimer),
  };
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

/// The projects whose checkouts' branches are being read, and those to read
/// again on the next draw.
const readingGit = new Set<string>();
export const gitDue = new Set<string>();

/// Reads each checkout's branch and distance from its upstream: the local refs
/// first, then again after a fetch. Kept apart from "where" so a slow fetch
/// never holds the page.
export async function loadGit(name: string) {
  if (readingGit.has(name)) return;
  readingGit.add(name);
  gitDue.delete(name);

  try {
    for (const fetch of [false, true]) await readGit(name, fetch);
  } finally {
    readingGit.delete(name);
  }
}

async function readGit(name: string, fetch: boolean) {
  try {
    const rows = await invoke<GitRow[]>("project_git_state", { project: name, fetch });
    state.gitsync.set(name, rows);
    keep("gitsync", name, rows);
  } catch (e) {
    console.warn("project git state:", e);
    if (!state.gitsync.has(name)) state.gitsync.set(name, []);
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
    fix: line.buttons && failed && agentCanFix(failed) ? repairButton(p.name, m.id, failed) : null };
}

export function missingMachine(p: Project, m: WhereRow) {
  const text = m.text || "", title = m.kind === "none" && m.checked_at ? `Checked ${formatAgo(m.checked_at, "since")}` : "";
  let action: { label: string; disabled: boolean; run(): void } | null = null;
  if (!state.home?.machines?.some(x => x.id === m.id && x.can_host === false)) {
    if (m.kind === "none" && m.add) action = { label: "Add", disabled: false, run: () => openAddToMachine({ project: p.name, machine: m, added: () => { refresh(); loadWhere(p.name); } }) };
    else if (m.kind === "none" && p.url) { const url = p.url; action = { label: "Clone here", disabled: false, run: () => importSheet(repoFromRemote(url), m.id) }; }
  }
  return { text, title, syncthing: m.kind === "none" ? m.syncthing ?? null : null, action };
}

/// Actions shared by a checkout's machine header and table row.
export function checkoutItems(p: Project, m: WhereRow, path: string): MenuItem[] {
  return [
    { label: "Start an agent here", on: () => go("pane", { agentnew: { prompt: "", at: { machine: m.id, cwd: path, places: projectPlaces(p), project: p.name }, assistant: false } }) },
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
      invoke<ProjectMachineRow[]>("project_machines", { project: name }),
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

/// The Sessions tab's list. It stays one host while the tabs switch, so its
/// virtual rows keep their scroll anchor and focus.
let sessionsTab: { project: string; host: HTMLElement } | null = null;

export function disposeSessionPanel() { if (sessionsTab) removeWindowPart("project-sessions:all"); sessionsTab = null; }

export function sessionsHost(project: string, rows: SessionRow[]) {
  if (sessionsTab?.project !== project) { disposeSessionPanel(); sessionsTab = { project, host: retainedHost("cc-panel") }; }
  showWindowParts([{ part: "project-sessions:all", host: sessionsTab.host, identity: project, content: <ProjectSessions rows={rows} all /> }]);
  return sessionsTab.host;
}

/// The project's past sessions, newest first, with the note that they are
/// being read again.
export function sessionsList(p: Project, running: RunningAgent[]): RecentSessions {
  const sessions = state.sessions.get(p.name);
  const past = projectPastSessions(sessions, running, [...moveStore.getState().records.values()]).filter(sessionVisible);
  if ((sessions === undefined || sessionsDue.has(p.name)) && p.imported) loadSessions(p.name);
  const freshness = sessions !== undefined ? pageFresh("sessions", p.name, readingSessions.has(p.name)) : null;
  if (!p.imported || sessions === undefined || !past.length) disposeSessionPanel();

  if (!p.imported) return { rows: null, empty: "Import this project to see its sessions.", count: past.length, freshness };
  if (sessions === undefined) return { rows: null, empty: "Reading sessions…", count: past.length, freshness };
  if (!past.length) return { rows: null, empty: "No past sessions yet.", count: past.length, freshness };
  return { rows: past, empty: null, count: past.length, freshness };
}

/// Project `name`'s sessions beside the one on screen, `current`: read when first shown.
export function projectRail(name: string, current: string | null) {
  const p = projectRows()?.find(row => row.name === name);
  if (!p) return null;
  const sessions = state.sessions.get(name), moves = [...moveStore.getState().records.values()];
  if ((sessions === undefined || sessionsDue.has(name)) && p.imported) loadSessions(name);
  const running = projectRunning(p, state.home?.running, sessions, moves).filter(sessionVisible);
  const past = projectPastSessions(sessions, running, moves).filter(sessionVisible);

  return <ProjectRail project={name} label={projectName(p)} running={running} past={past} current={current} back={() => void openProjects(name)} />;
}

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
  if (shell.getState().side.rail === name) drawRail();
  if (currentLocation()?.view !== "projects" || state.picked !== name) return;
  // The draw also drops the Updating note.
  drawProjects();
}

/// The checkouts the machines found that are not projects yet, one line
/// each with Import; the sheet lists them all.
export function foundSection(enabled: boolean) {
  if (!enabled || !state.found?.length) return null;
  return <div id="sec-found"><FoundSection rows={state.found} importing={state.importing} error={state.foundError} importOne={importFromPage} add={addProject} /></div>;
}

let askedGit = false;
/// The open PRs and issues in repos without a project, each repo with
/// Import project, under the Projects overview.
export function elsewhereSection(rows: Project[], enabled: boolean) {
  if (!enabled) return null;
  if (!state.git) {
    if (!askedGit) { askedGit = true; refreshGit().then(redrawProjects); }
    return null;
  }

  const repos = outsideThreads(state.git, rows).slice(0, 8);
  if (!repos.length) return null;
  const body = repos.map((r, i) => {
    // A checkout the machines found imports as it is; otherwise the sheet clones.
    const here = foundFor(state.found, r.repo);
    const failed = here && state.foundError?.name === here.name ? state.foundError.message : null;
    const importing = !!here && state.importing === here.name;
    return { key: `${r.host} ${r.repo}`, name: r.repo, failed, last: r.last, open: i === 0, items: r.items, start: null,
      action: { label: importing ? "Importing…" : "Import project", primary: i === 0, disabled: !!here && state.importing != null, run: () => { if (here) void importFromPage(here); else importSheet(r.repo); } } };
  });
  return <div id="sec-elsewhere"><RepoSection id="section:elsewhere" title="Your PRs and issues elsewhere" note="in repos without a project" repos={body} sort="default" open={openUrl} /></div>;
}

// Reopened sessions and moves change independently of project membership. The project's page
// shows them, and the rail beside one of its sessions.
const shownSessions = () => currentLocation()?.view === "projects" ? state.picked : shell.getState().side.rail;
const projectSessionsShown = () => !!shownSessions();
async function refreshProjectSessions() {
  const name = shownSessions();
  if (name) { sessionsDue.add(name); await loadSessions(name); }
}
// Agents report many times a second during turns; the list follows every two seconds.
subscribeChanges([topic("agents"), topic("moves")], { deliver: "latest", receive: refreshProjectSessions }, { when: projectSessionsShown, wait: SLOW_MS });
listen("agent-title-changed", () => {
  if (projectSessionsShown()) void refreshProjectSessions().catch(e => console.warn("project session titles:", e));
});
