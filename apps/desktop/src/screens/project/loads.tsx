import { MACHINES } from "../../stores/machines/machine-store.ts";
import { projectChanged } from "../../stores/project/barstate.ts";
import { removeWindowPart, showWindowParts } from "../../parts/center/window-root.tsx";
import { ProjectSessions } from "./sessions-view.tsx";
import { retainedHost } from "./page-view.tsx";
import { moveStore } from "../../stores/agents/move-store.ts";
import { subscribeEach, topic } from "../../ui/mesh-events.ts";
import { pushedEach } from "../../ui/snapshot-state.ts";
// Reading a project page: its machines, lists and sessions, kept fresh
// while the user moves between projects, and prefetched ahead.
import { sessionVisible } from "../../stores/agents/closing.ts";
import { formatAgo, reason } from "../../ui/format.ts";
import { copy } from "../../ui/clipboard.ts";
import { type MenuItem } from "../../ui/menu.tsx";
import { Changes, UNWATCHED_MS } from "../../ui/pagecache.ts";
import { parseRecent, prefetchOrder, touchRecent } from "../../stores/project/projectloads.ts";
import { invoke, listen, openUrl } from "../../ui/tauri.ts";
import { FoundSection, RepoSection } from "./outside-lists.tsx";
import { ProjectRail, type RailCard, type RecentSessions } from "./split-view.tsx";
import { hideRail } from "../../parts/center/window-shell.tsx";
import { foundFor, withSyncthing, projectName, projectPastSessions, type Project, type ProjectMachineRow, type SessionRow, type SyncthingPlace, type WhereRow, whereRows } from "../../stores/project/projects-state.ts";
import { repoFromRemote } from "../../stores/addproject/importproject-state.ts";
import { outsideThreads } from "../../stores/review/branchwork.ts";
import { localProjectId, projectRef } from "../../stores/project/localid.ts";
import { machineTile } from "./board.ts";
import { state, pages, keep, pageFresh, whereOf, sessionsOf, gitOf, sessionsDue, projectLoaded } from "../../stores/center/state.ts";
import { refreshGit } from "../../parts/center/openitems.ts";
import { currentLocation, go, windowAlone } from "../../parts/center/nav.ts";
import { projectHover } from "../../parts/project/projectops.ts";
import { projectRows, projectPlaces } from "../../stores/project/projectrows.ts";
import { sessionChanges, loadSessions, readingSessions, railSessions } from "../../parts/project/sessionsload.ts";
import { importSheet, addProject, importFromPage, loadFound, openProjects } from "../../parts/project/addflow.ts";
import { fsync, runs, repairButton } from "../../parts/project/sections.ts";
import { terms, thr } from "../../parts/project/stores.ts";
import { onScreen } from "../../parts/center/refresh.ts";
import { openAddToMachine } from "./addmachine.tsx";
import { type RunningAgent } from "../../stores/agents/running-state.ts";
import { agentCanFix, gitLine, movingLabel, moveKey, moveTitle, withRow, type GitMove, type GitRow } from "../../stores/project/gitsync-state.ts";
import { openHandoff } from "./handoff.tsx";
import { perfFresh } from "../../ui/perf.ts";

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
const foundChanges = new Changes();
subscribeEach([topic("projects"), topic("machines"), topic("sync")], event => { whereChanges.changed(); takeWhere(event.data); });
// A checkout's git state changed in the background and arrives with the event.
listen<{ project: string; row: GitRow }>("project-git-changed", ({ payload }) => {
  // A project not read yet has no list to amend; its first read has the row.
  if (gitOf(payload.project) === undefined) return;
  rememberGitRow(payload.project, payload.row);
  if (currentLocation()?.view === "projects" && state.picked === payload.project) projectLoaded.notify();
});
subscribeEach(topic("projects"), () => foundChanges.changed());

/// Marks what opening project `name` reads behind its kept page.
export function markDue(name: string) {
  if (whereChanges.dirty(name)) whereDue.add(name);
  if (sessionChanges.dirty(name)) sessionsDue.add(name);
  if (pages.due("gitsync", name, UNWATCHED_MS)) gitDue.add(name);
}

/// Reads project `name`'s terminals the first time; the `terminals` push keeps them after.
function loadTerms(name: string) {
  if (terms.list(name) !== undefined) return;
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
  fsync.ensure(name);
  if (sessionsDue.has(name) || sessionsOf(name) === undefined) reads.push(loadSessions(name));
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
  const reads: (Promise<unknown> | undefined)[] = [pages.due("terminals", name) ? loadTerms(name) : undefined, fsync.ensure(name), thr.ensure(id), runs.ensure(id)];

  if (whereChanges.dirty(name) && pages.due("where", name)) reads.push(loadWhere(name));
  if (sessionChanges.dirty(name) && pages.due("sessions", name)) reads.push(loadSessions(name));
  return Promise.allSettled(reads);
}

/// How long the pointer rests on a project before it is read ahead.
const HOVER_MS = 150;
/// How long after a project opens, the window idle, the projects opened
/// last are read ahead.
const IDLE_PREFETCH_MS = 4000;

/// Reads project `name` ahead once the pointer rests on it.
/// The pointer rests on one element at a time, so one timer serves them all.
let hoverTimer: ReturnType<typeof setTimeout> | undefined;
function prefetchOnHover(name: string | null) {
  clearTimeout(hoverTimer);
  if (name !== null) hoverTimer = setTimeout(() => prefetch(name), HOVER_MS);
}
projectHover.follow(prefetchOnHover);

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

/// The rows the machines pushed while a read of a project's git state was out, by machine: they
/// are newer than what the read answers.
const pushedDuring = new Map<string, Map<string, GitRow>>();

async function readGit(name: string, fetch: boolean) {
  pushedDuring.set(name, new Map());

  try {
    const read = await invoke<GitRow[]>("project_git_state", { project: name, fetch });
    const newer = pushedDuring.get(name);
    keep("gitsync", name, read.map(row => newer?.get(row.machine_id) ?? row));
  } catch (e) {
    console.warn("project git state:", e);
    if (gitOf(name) === undefined) pages.hold("gitsync", name, []);
  }

  pushedDuring.delete(name);

  if (currentLocation()?.view === "projects" && state.picked === name) projectLoaded.notify();
}

/// A running move's direction and a refused one's reason, by project and machine.
const moves = new Map<string, { moving: GitMove | null; failed?: string }>();

export function rememberGitRow(name: string, row: GitRow) {
  pushedDuring.get(name)?.set(row.machine_id, row);
  keep("gitsync", name, withRow(gitOf(name), row));
}

/// Pulls or pushes on one machine; the counts come back with the answer.
async function gitMove(p: Project, m: WhereRow, row: GitRow, move: GitMove) {
  const key = moveKey(p.name, m.id);
  if (moves.get(key)?.moving || !row.branch) return;
  moves.set(key, { moving: move });
  projectLoaded.notify();

  try {
    const next = await invoke<GitRow>("project_git_move", { project: p.name, machineId: m.id, branch: row.branch, direction: move });
    moves.delete(key);
    rememberGitRow(p.name, next);
  } catch (e) {
    moves.set(key, { moving: null, failed: reason(e) });
  }

  projectLoaded.notify();
}

export function gitControls(p: Project, m: WhereRow, online: boolean) {
  const row = gitOf(p.name)?.find(r => r.machine_id === m.id);
  if (!row) return null;
  const line = gitLine(row), move = moves.get(moveKey(p.name, m.id)), failed = move?.failed ?? null;
  const buttons = line.buttons ? (["pull", "push"] as const).map(direction => {
    const n = direction === "pull" ? row.behind : row.ahead, active = direction === "pull" ? line.pull : line.push;
    return { direction, count: n, moving: move?.moving === direction, title: move?.moving === direction ? movingLabel(direction) : moveTitle(direction, n), label: moveTitle(direction, n), disabled: !online || !!move?.moving || !active, run: () => void gitMove(p, m, row, direction) };
  }) : [];
  return { line, buttons, failed: line.buttons ? failed : null,
    handoff: line.buttons && p.machines.length > 1 && !m.syncthing ? () => openHandoff(p, m.id, () => loadGit(p.name)) : null,
    fix: line.buttons && failed && agentCanFix(failed) ? repairButton(p.name, m.id, failed) : null };
}

export function missingMachine(p: Project, m: WhereRow) {
  const text = m.text || "", title = m.kind === "none" && m.checked_at ? `Checked ${formatAgo(m.checked_at, "since")}` : "";
  let action: { label: string; disabled: boolean; run(): void } | null = null;
  if (!state.home?.machines?.some(x => x.id === m.id && x.can_host === false)) {
    if (m.kind === "none" && m.add) action = { label: "Add", disabled: false, run: () => openAddToMachine({ project: p.name, machine: m }) };
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

/// The checkouts and Syncthing places the pushes carried, by project: the pushes of the three
/// topics each carry some of the views, and a row is drawn from both.
const pushedMachines = new Map<string, ProjectMachineRow[]>();
const pushedPlaces = new Map<string, SyncthingPlace[]>();
export const readingWhere = new Set<string>();

/// Takes the checkouts an update pushed for the projects native follows. A project it pushed
/// nothing for stays marked changed, and reads when opened.
function takeWhere(data: unknown) {
  const names = new Set<string>();

  for (const { args, pushed } of pushedEach<ProjectMachineRow[]>(data, "project_machines")) {
    if (typeof args.project !== "string") continue;
    if ("error" in pushed) { console.warn("project machines:", pushed.error); continue; }
    pushedMachines.set(args.project, pushed.value);
    names.add(args.project);
  }

  for (const { args, pushed } of pushedEach<SyncthingPlace[]>(data, "project_syncthing")) {
    if (typeof args.project !== "string") continue;
    if ("error" in pushed) { console.warn("project syncthing:", pushed.error); pushedPlaces.set(args.project, []); } else pushedPlaces.set(args.project, pushed.value);
    names.add(args.project);
  }

  for (const name of names) {
    const machines = pushedMachines.get(name);
    if (!machines) continue;
    whereChanges.read(name);
    settleWhere(name, withSyncthing(machines, pushedPlaces.get(name) ?? []), whereOf(name));
  }
}
/// Projects whose kept places are due another read on the next draw.
export const whereDue = new Set<string>();
const whereReads = new Map<string, Promise<void>>();
/// Reads project `name`'s checkouts once: the first time, before a push brings them.
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
  const before = whereOf(name);
  // A kept list shows a spinner in its header meanwhile.
  if (before && state.picked === name) queueMicrotask(() => projectChanged.notify());
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

  settleWhere(name, got, before);
}

/// Keeps `got`, the checkouts of project `name`, over what `before` held.
function settleWhere(name: string, got: ProjectMachineRow[] | null, before: ProjectMachineRow[] | null | undefined) {
  // A failed read keeps the last list it had.
  // Rows still checking show, and are kept once they settle.
  if (got && !got.some(m => m.checking)) keep("where", name, got);
  else if (got || !before) pages.hold("where", name, got);
  const folders = (list: ProjectMachineRow[] | null) => (list || []).filter(m => m.folder).map(m => `${m.id} ${m.folder}`).join("\n");

  if (got && before !== undefined && folders(before) !== folders(got)) {
    sessionsDue.add(name);
  }

  // The header's Open in follows this Mac's checkout.
  if (state.picked === name) projectLoaded.notify();
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
  const sessions = sessionsOf(p.name);
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
  const sessions = railSessions(name);
  if (!sessions) return null;

  return <ProjectRail project={name} label={projectName(sessions.project)} running={sessions.running} past={sessions.past} current={current} card={railCard(sessions.project)} back={() => { if (!windowAlone) hideRail(); void openProjects(name); }} />;
}

/// What the rail's card shows of a project, as its page's rows count them: open pull requests
/// and issues, Actions and machines. Reads what is missing, as the project page does.
function railCard(p: Project): RailCard | null {
  if (!p.imported) return null;
  const machines = whereOf(p.name);
  if (machines === undefined || whereDue.has(p.name)) loadWhere(p.name);
  const pid = projectRef(p.name).project_id;
  thr.ensure(pid);
  const actions = runs.tile(pid), sync = state.sync.filter(row => row.project === p.name);
  return {
    prs: thr.prs(pid)?.length ?? null, issues: thr.issues(pid)?.length ?? null,
    actions: actions.count, failed: actions.rows[0]?.chip?.text === "failed",
    machines: machineTile(whereRows(p, machines || null, MACHINES.mac?.id || null), sync).count,
  };
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
    if (!askedGit) { askedGit = true; refreshGit().then(() => projectChanged.notify()); }
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

