// Reading a project page: its view, agent, machines and sessions, kept
// fresh while the user moves between projects, and prefetched ahead.
import { ago, base, reason, since } from "../ui/format.ts";
import { copyButton, dbtn as button, emptyPanel, fitRows, freshNote } from "../ui/parts.ts";
import { MACHINE_REFRESH_MS, SWITCH_MS } from "../ui/pagecache.ts";
import { keyedReads, parseRecent, prefetchOrder, touchRecent } from "./projectloads.ts";
import { invoke } from "../ui/tauri.ts";
import { harnessEl } from "../ui/harnessicon.ts";
import { foundRow } from "../addproject/addproject.ts";
import { foundFor, withSyncthing, agentLabel, pastSessions, nextSessions, type Project, type ProjectMachineRow, type SessionRow, type SyncthingPlace, type WhereRow } from "./projects-state.ts";
import { repoFromRemote } from "../addproject/importproject-state.ts";
import { el as h } from "../ui/dom.ts";
import { ITEMS_SHOWN, repoUrl } from "../center/openitems-state.ts";
import { outsideThreads } from "../review/branchwork.ts";
import { agentFor, pageAgent, type ProjectAgent, type ProjectView } from "./agent.ts";
import { type Started, state, pages, keep, pageFresh } from "../center/state.ts";
import { failure, section, rowButton } from "../ui/pieces.ts";
import { openUrl, openItemRow, repoLine, refreshGit } from "../center/openitems.ts";
import { shown, go } from "../center/views.ts";
import { projectRows, importSheet, addProject, importFromPage } from "../center/sidebar.ts";
import { terms, fsync, revs, thr, runs, projectsEl, redrawProjects, redrawProjectsPage } from "./sections.ts";
import { drawProjects } from "./page.ts";
import { refresh, onScreen } from "../center/refresh.ts";
import { machineSync } from "../machines/machine-state.ts";
import type { RunningAgent } from "../agents/running-state.ts";

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
  shown: () => (shown === "projects" ? state.picked : null),
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

/// On opening project `name`, behind what the page drew from the cache:
/// its view at once rather than after Home, and what the server keeps of
/// its code host once that is `SWITCH_MS` old, so issues closed or opened
/// and new runs show. The drawing asks for the rest as it is due.
export function revalidate(name: string) {
  loadView(name);
  const agent = pageAgent(state.agents, name, state.subscribed);
  if (agent) {
    thr.ensure(agent.project_id, SWITCH_MS);
    runs.ensure(agent.project_id, SWITCH_MS);
  }
  terms.ensure(name);
  fsync.ensure(name);
  if (sessionsDue.has(name) || !state.sessions.has(name)) loadSessions(name);
}

/// Reads project `name` ahead, as opening it would, without drawing it:
/// only what is due, so pointing at projects never asks more often than
/// opening them would.
function prefetch(name: string): Promise<unknown> {
  const p = (projectRows() || []).find(r => r.name === name);
  if (!p?.imported) return Promise.resolve();
  const agent = pageAgent(state.agents, name, state.subscribed);
  const reads: (Promise<unknown> | undefined)[] = [revs.ensure(name), terms.ensure(name), fsync.ensure(name, MACHINE_REFRESH_MS)];
  if (agent) {
    if (agentFor(state.agents, name) && pages.due("view", name, MACHINE_REFRESH_MS)) reads.push(loadView(name));
    // The code host's lists at their own pace: an ask keeps the server
    // reading the host for that project a while.
    reads.push(thr.ensure(agent.project_id), runs.ensure(agent.project_id));
  }
  if (pages.due("where", name)) reads.push(loadWhere(name));
  if (pages.due("sessions", name)) reads.push(loadSessions(name));
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
    if (!copy) importSheet(false, repoFromRemote(p.url), m.id);
    else if (!copy.from) copies.set(key, { state: "already" });
    refresh();
    await loadWhere(p.name);
  } catch (e) {
    copies.set(key, { state: "failed", text: reason(e) });
  }
  redrawProjectsPage();
}

/// One machine's row under "Where it lives": its folder and what to do
/// there, or why there is none.
export function whereRow(p: Project, m: WhereRow) {
  const name = h("span", "nm", h("b", "", m.name));
  if (m.kind === "here") {
    const path = m.folder!;
    const start = button("Start an agent here", "small", () => go("pane", { agentnew: { prompt: "", at: { machine: m.id, cwd: path, places: projectPlaces(p), project: p.name } } }));
    const copy = copyButton(path, "Copy path", "dbtn quiet small");
    const finder = m.mac ? button("Open in Finder", "quiet small", () => invoke("open_folder", { path }).catch(e => console.warn("open folder:", e))) : null;
    const folder = h("span", "nm", h("span", "os", path));
    folder.title = path;
    const moving = machineSync(m.id, state.sync.filter(s => s.project === p.name));
    const said = moving ? (moving.pct == null ? "Scanning" : `Syncing ${moving.pct}%`) : copies.get(`${p.name}\n${m.id}`)?.state === "already" ? "Already here" : "";
    if (said) folder.append(" ", h("small", "", said));
    if (m.syncthing) {
      // Syncthing keeps it in step, so Beep's own sync stays off here.
      const note = h("small", "", "Synced by Syncthing");
      note.title = `Syncthing folder "${m.syncthing}". Beep does not sync it here.`;
      folder.append(" ", note);
    }
    return [name, folder, h("span", "acts", finder, copy, start)];
  }
  const key = `${p.name}\n${m.id}`;
  const copied = copies.get(key);
  const said = h("span", "os", copied?.text || m.text);
  if (m.kind === "none" && m.checked_at) said.title = `Checked ${since(m.checked_at)}`;
  const what = h("span", "nm", said);
  if (m.kind === "none" && m.copy && copied?.state !== "failed") {
    const copy = button(copied ? "Copying…" : "Copy here", "small", () => copyHere(p, m, key));
    copy.disabled = !!copied;
    return [name, what, h("span", "acts", copy)];
  }
  const clone = m.kind === "none" && p.url ? button("Clone here", "small", () => importSheet(false, repoFromRemote(p.url), m.id)) : null;
  if (clone) return [name, what, h("span", "acts", clone)];
  // Nothing to do there, so the note takes the buttons' room too.
  what.classList.add("wide");
  return [name, what];
}

/// Asks the server where `name` lives on each machine; while it looks through
/// one, it asks again shortly.
export const readingWhere = new Set<string>();
let whereTimer: ReturnType<typeof setTimeout> | undefined;
/// Projects whose kept places are due another read on the next draw.
export const whereDue = new Set<string>();
export async function loadWhere(name: string) {
  if (readingWhere.has(name)) return;
  readingWhere.add(name);
  whereDue.delete(name);
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
  if (state.picked === name) clearTimeout(whereTimer);
  if (got?.some(m => m.checking) && state.picked === name) {
    whereTimer = setTimeout(() => {
      if (shown === "projects" && state.picked === name) loadWhere(name);
    }, 3000);
  }
  // The header's Open in follows this Mac's checkout.
  if (state.picked === name) redrawProjectsPage();
}

/// The sessions your coding agents ran in the project's folders, newest first,
/// each opens; those running now are under Agents.
const SESSIONS_SHOWN = 5;

export function sessionsSection(p: Project, running: RunningAgent[]) {
  const sessions = state.sessions.get(p.name);
  if ((sessions === undefined || sessionsDue.has(p.name)) && p.imported) loadSessions(p.name);
  let body;
  if (!p.imported) body = emptyPanel(h("span", "", "Import this project to see its sessions."));
  else if (sessions === undefined) body = emptyPanel(h("span", "", "Reading sessions…"));
  else {
    const past = pastSessions(sessions, running);
    body = past.length
      ? h("div", "cc-panel", fitRows(past.map(sessionRow), SESSIONS_SHOWN))
      : emptyPanel(h("span", "", "No sessions yet."));
  }
  const note = sessions !== undefined ? freshNote(pageFresh("sessions", p.name, readingSessions.has(p.name))) : null;
  note?.classList.add("ss-updating");
  return section("sec-sessions", "Sessions", sessions?.length || "", null, body, note);
}

/// The agent view for a session row.
function openAgentRow(r: SessionRow) { go("pane", { agent: { kind: "agent", ...r } }); }

function sessionRow(s: SessionRow) {
  const tile = harnessEl(s.agent);
  tile.title = agentLabel(s.agent);
  const meta = [tile, s.status, s.machine_name, base(s.cwd), s.last_activity ? ago(s.last_activity) : ""]
    .filter(Boolean).map(x => typeof x === "string" ? h("span", "", x) : x);
  const row = h("button", "pv-row", h("span", "t", s.title || "Untitled session"), h("span", "m", ...meta));
  row.addEventListener("click", () => openAgentRow(s));
  return row;
}

/// The projects whose sessions are being read, and those to read again on the
/// next draw.
const readingSessions = new Set<string>();
export const sessionsDue = new Set<string>();
export async function loadSessions(name: string) {
  if (readingSessions.has(name)) return;
  readingSessions.add(name);
  sessionsDue.delete(name);
  let got: SessionRow[] | null = null;
  try {
    got = await invoke<SessionRow[]>("project_sessions", { project: name });
  } catch (e) {
    console.warn("project sessions:", e);
  } finally {
    readingSessions.delete(name);
  }
  const { list, changed } = nextSessions(state.sessions.get(name), got);
  state.sessions.set(name, list);
  if (got) keep("sessions", name, list);
  if (shown !== "projects" || state.picked !== name) return;
  if (changed) drawProjects(projectsEl());
  else for (const el of document.querySelectorAll(".ss-updating")) el.remove();
}

/// The checkouts the machines found that are not projects yet: the first
/// three, each with Import; the sheet lists them all.
export function foundSection() {
  if (!state.found?.length) return null;
  const list = state.found.slice(0, 3).map(row => foundRow(row,
    { busy: state.importing, error: state.foundError?.name === row.name ? state.foundError.message : null }, importFromPage));
  const more = state.found.length > 3 ? button(`All ${state.found.length}`, "quiet small", addProject) : null;
  return section("sec-found", "Found on your machines", "", more, h("div", "cc-panel ap-list", ...list));
}

let askedGit = false;
/// The open PRs and issues in repos without a project, each repo with
/// Import project, under the Projects overview.
export function elsewhereSection(rows: Project[]) {
  if (!state.git) {
    if (!askedGit) { askedGit = true; refreshGit().then(redrawProjects); }
    return null;
  }
  const repos = outsideThreads(state.git, rows).slice(0, 8);
  if (!repos.length) return null;
  const body = repos.flatMap((r, i) => {
    // A checkout the machines found imports as it is; otherwise the sheet clones.
    const here = foundFor(state.found, r.repo);
    const imp = rowButton(here && state.importing === here.name ? "Importing…" : "Import project", i ? "small" : "pri small", () => here ? importFromPage(here) : importSheet(false, r.repo));
    imp.disabled = !!here && state.importing != null;
    const failed = here && state.foundError?.name === here.name ? state.foundError.message : null;
    const sub = failed ?? (here ? `on ${here.places.map(p => p.machine_name).join(", ")}` : r.host);
    const head = repoLine(undefined, r.repo, sub, [h("span", "age", ago(r.last)), imp], () => openUrl(repoUrl(r.items[0].url)));
    return [head, ...r.items.slice(0, ITEMS_SHOWN).map(item => openItemRow(item, null))];
  });
  return section("sec-elsewhere", "Your open PRs and issues in repos without a project", "", null,
    h("div", "cc-panel cc-open", ...body));
}

/// "github.com" from a project's web page.
export function hostOf(url: string | null) {
  try { return new URL(url ?? "").host; } catch { return ""; }
}
