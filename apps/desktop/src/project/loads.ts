// Reading a project page: its view, agent, machines and sessions, kept
// fresh while the user moves between projects, and prefetched ahead.
import { ago, since } from "../ui/format.ts";
import { dbtn as button, emptyPanel, fitRows, freshNote } from "../ui/parts.ts";
import { MACHINE_REFRESH_MS, SWITCH_MS } from "./pagecache.ts";
import { keyedReads, parseRecent, prefetchOrder, touchRecent } from "./projectloads.ts";
import { invoke } from "../ui/tauri.ts";
import { base, type RunningAgent } from "../center/home-state.ts";
import { foundRow, injectStyle as injectAddProjectStyle } from "../addproject/addproject.ts";
import { withSyncthing, agentLabel, pastSessions, nextSessions, type Project, type ProjectMachineRow, type SessionRow, type SyncthingPlace, type WhereRow } from "./projects-state.ts";
import { repoFromRemote } from "../addproject/importproject-state.ts";
import { el as h } from "../ui/dom.ts";
import { ITEMS_SHOWN, repoUrl } from "../center/openitems-state.ts";
import { outsideThreads } from "../review/branchwork.ts";
import { agentFor, type ProjectAgent, type ProjectView } from "./agent.ts";
import { type Started, state, pages, keep, pageFresh } from "../center/state.ts";
import { failure, section, rowButton } from "../center/pieces.ts";
import { openUrl, openItemRow, repoLine, refreshGit } from "../center/openitems.ts";
import { shown, go } from "../center/views.ts";
import { projectRows, importSheet, addProject, importFromPage } from "../center/sidebar.ts";
import { terms, fsync, revs, thr, runs, projectsEl, redrawProjects, redrawProjectsPage } from "./sections.ts";
import { drawProjects } from "./page.ts";
import { refresh, onScreen } from "../center/refresh.ts";
import { reason } from "../addproject/importproject-state.ts";

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
  const agent = agentFor(state.agents, name);
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
  const agent = agentFor(state.agents, name);
  const reads: (Promise<unknown> | undefined)[] = [revs.ensure(name), terms.ensure(name), fsync.ensure(name, MACHINE_REFRESH_MS)];
  if (agent) {
    if (pages.due("view", name, MACHINE_REFRESH_MS)) reads.push(loadView(name));
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

/// One machine's row under "Where it lives": its folder and what to do
/// there, or why there is none.
export function whereRow(p: Project, m: WhereRow) {
  const name = h("span", "nm", h("b", "", m.name));
  if (m.kind === "here") {
    const path = m.folder!;
    const start = button("Start an agent here", "small", () => go("pane", { agentnew: { prompt: "", at: { machine: m.id, cwd: path, places: projectPlaces(p), project: p.name } } }));
    const copy = button("Copy path", "quiet small", async () => {
      try { await navigator.clipboard.writeText(path); copy.textContent = "Copied"; } catch (e) { console.warn("copy:", e); }
    });
    const finder = m.mac ? button("Open in Finder", "quiet small", () => invoke("open_folder", { path }).catch(e => console.warn("open folder:", e))) : null;
    const folder = h("span", "nm", h("span", "os", path));
    folder.title = path;
    if (m.syncthing) {
      // Syncthing keeps it in step, so Beep's own sync stays off here.
      const note = h("small", "", "Synced by Syncthing");
      note.title = `Syncthing folder "${m.syncthing}". Beep does not sync it here.`;
      folder.append(" ", note);
    }
    return [name, folder, h("span", "acts", finder, copy, start)];
  }
  const said = h("span", "os", m.text);
  if (m.kind === "none" && m.checked_at) said.title = `Checked ${since(m.checked_at)}`;
  const what = h("span", "nm", said);
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
/// each resumable; those running now are under Agents.
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
  const ended = s.status === "ended";
  const said = h("span", "bad");
  const act: HTMLButtonElement = ended
    ? button("Resume", "small", async () => {
      act.disabled = true;
      act.textContent = "Resuming…";
      said.textContent = "";
      try {
        const started = await invoke<Started>("agent_resume", { machineId: s.machine_id, agent: s.agent, session: s.id, prompt: null });
        sessionsDue.add(state.picked!);
        openAgentRow(started.session);
      } catch (e) {
        act.disabled = false;
        act.textContent = "Resume";
        said.textContent = reason(e);
      }
    })
    : button("Open", "small", () => openAgentRow(s));
  const meta = [h("span", "ap-ag", agentLabel(s.agent)), ended ? "ended" : s.status, s.machine_name, base(s.cwd), s.last_activity ? ago(s.last_activity) : ""]
    .filter(Boolean).map(x => typeof x === "string" ? h("span", "", x) : x);
  return h("div", "pv-row", h("span", "t", s.title || "Untitled session"), h("span", "m", ...meta), h("span", "acts", said, act));
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
  injectAddProjectStyle();
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
    const imp = rowButton("Import project", i ? "small" : "pri small", () => importSheet(false, r.repo));
    const head = repoLine(undefined, r.repo, r.host, [h("span", "age", ago(r.last)), imp], () => openUrl(repoUrl(r.items[0].url)));
    return [head, ...r.items.slice(0, ITEMS_SHOWN).map(item => openItemRow(item, null))];
  });
  return section("sec-elsewhere", "Your open PRs and issues in repos without a project", "", null,
    h("div", "cc-panel cc-open", ...body));
}

/// "github.com" from a project's web page.
export function hostOf(url: string | null) {
  try { return new URL(url ?? "").host; } catch { return ""; }
}
