import { createElement } from "react";
import { removeWindowPart, showWindowParts } from "../center/window-root.ts";
import { ProjectRunning } from "./running-view.tsx";
import { ProjectsOverview } from "./overview-view.tsx";
import { ProjectFallback } from "./split-view.tsx";
import { MV } from "../agents/move-store.ts";
// The Projects page's overview: the list, groups and cards of every
// project, and a picked project's detail sections.
import { sessionVisible } from "../agents/closing.ts";
import { formatAgo, plural } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import { awaitsView, projectActivity, projectRunning, whereRows, whereCount, overviewGroups, syncedBy, type OverviewCard, type Project, plainPath, type ProjectMachineRow, type WhereRow } from "./projects-state.ts";
import { el as h } from "../ui/dom.ts";
import { makeCustomAgents } from "./customagents.ts";
import { type AgentsPlace, type CustomAgentRow, agentPrompt, agentsPlace } from "./customagents-state.ts";
import { pages, state, warm, pageFresh } from "../center/state.ts";
import { showMachineMenu } from "../machines/table.ts";
import { MachinesSection, type ProjectMachineItem } from "./machines-section-view.tsx";
import { blocked, confirmRemove, removeMenuItems } from "./removemachine.ts";
import { changeFolder, changeFolderItems } from "./changefolder.ts";
import { addProject } from "../center/sidebar.ts";
import { splitOn, projectSplit, disposeProjectShell } from "./split.ts";
import { EmptyPanel, ErrorNote, hostPart, viewPart, type PagePart } from "./page-view.tsx";
import { redrawProjects, redrawProjectsPage, pc, terms, fsync } from "./sections.ts";
import { drawProjectComp, fillBar } from "./composer.ts";
import { missingMachine, gitControls, checkoutItems, readingWhere, whereDue, loadWhere, gitDue, loadGit, sessionsList } from "./loads.ts";
import { terminalNote, terminalWhere } from "../terminals/terminals.ts";
import { go } from "../center/views.ts";
import { type RunningAgent } from "../agents/running-state.ts";
import { single } from "../ui/active.ts";

/// Every project as a card, grouped by org: its tasks per state, its
/// workers, what changed last and where it lives.
let overviewHost: HTMLElement | null = null;

export function disposeOverview() {
  if (!overviewHost) return;
  removeWindowPart("project-overview");
  overviewHost = null;
}

export function overviewSection(rows: Project[]): PagePart[] {
  visiting = null;
  const groups = overviewGroups(rows, state.overview, state.top, state.groups, overviewOrder(rows));
  if (!overviewHost) { overviewHost = h("div"); overviewHost.style.display = "contents"; }
  showWindowParts([{ part: "project-overview", host: overviewHost, content: createElement(ProjectsOverview, { groups, editing: state.renaming,
    sync: (name: string) => syncedBy(name, state.sync, state.where.get(name) ?? pages.get<ProjectMachineRow[]>("where", name)), add: addProject,
  }) }]);
  return [hostPart(overviewHost)];
}

/// When the user last opened each project, kept between launches.
const VISITS_KEY = "beep.projects.visited";
let visits = new Map<string, number>();
try {
  const saved: unknown = JSON.parse(localStorage.getItem(VISITS_KEY) ?? "{}");
  if (saved && typeof saved === "object") visits = new Map(Object.entries(saved).filter((e): e is [string, number] => typeof e[1] === "number"));
} catch { /* none kept */ }
let visiting: string | null = null;

function noteVisit(name: string) {
  if (visiting === name) return;
  visiting = name;
  visits.set(name, Date.now());
  try { localStorage.setItem(VISITS_KEY, JSON.stringify(Object.fromEntries(visits))); } catch { /* kept for this launch only */ }
}

/// The overview's recency, taken once per look at it: when projects come or
/// go, after a project was opened, and on a later draw while the pointer and
/// focus are elsewhere. Ambient updates do not move cards under the user.
let ranked: { names: string; at: number; activity: Map<string, number> } | null = null;
const RANK_MS = 60_000;

function overviewOrder(rows: readonly Project[]) {
  const names = rows.map(p => p.name).join("\n");
  const busy = !!overviewHost && (overviewHost.matches(":hover") || overviewHost.contains(document.activeElement));
  if (ranked && ranked.names === names && (busy || Date.now() - ranked.at < RANK_MS)) return ranked.activity;

  const sums = new Map((state.overview ?? []).map(o => [o.name, o]));
  const activity = new Map(rows.map(p => [p.name, projectActivity(p, state.sessions.get(p.name), visits.get(p.name), sums.get(p.name))]));
  ranked = { names, at: Date.now(), activity };
  return activity;
}

/// Optional task summaries for the cards. Local projects draw before this
/// read and remain visible if it fails or the server has no summaries.
export const loadOverview = single(async () => {
  try {
    state.overview = await invoke<OverviewCard[] | null>("project_overview");
  } catch (e) {
    console.warn("project overview:", e);
  }

  if (!state.picked) redrawProjectsPage();
});

/// One project: where it lives on every machine, with "Start an agent here"
/// per folder and "Copy here" where it is not, and the coding agents
/// running in it now.
export function projectDetail(p: Project): PagePart[] {
  noteVisit(p.name);
  ranked = null;
  if (p.imported) warm(p.name);
  const machines = p.imported ? state.where.get(p.name) : null;
  if (p.imported && (machines === undefined || whereDue.has(p.name))) loadWhere(p.name);
  if (p.imported && (!state.gitsync.has(p.name) || gitDue.has(p.name))) loadGit(p.name);
  const rows = whereRows(p, machines || null, state.mac?.id || null);
  const running = projectRunning(p, state.home?.running, state.sessions.get(p.name), [...MV.records.values()]).filter(sessionVisible);
  if (splitOn(p)) { disposeProjectFallback(); return projectSplit(p, rows, !!machines, running); }
  disposeProjectShell();

  // The project agent's view is on its way, or who has an agent is not known yet.
  if (awaitsView(p, state.agents)) {
    disposeProjectFallback();
    const error = state.pview.get(p.name)?.error;
    return [error
      ? viewPart("error", createElement(ErrorNote, { text: `Could not read the project: ${error}` }))
      : viewPart("reading", createElement(EmptyPanel, { text: "Reading the project…" }))];
  }

  // Without a project agent there are no tasks, runs or code host lists.
  if (fallback?.project !== p.name) { disposeProjectFallback(); fallback = { project: p.name, host: h("div", "pj-cols pj-half") }; }
  showWindowParts([{ part: "project-fallback", host: fallback.host, identity: p.name, content: createElement(ProjectFallback, {
    session: sessionsList(p, running, false),
    machines: whereSection(p, rows, !!machines), running: agentsSection(running), agents: customAgentsSection(rows),
  }) }]);
  return [hostPart(fallback.host)];
}

let fallback: { project: string; host: HTMLElement } | null = null;
export function disposeProjectFallback() {
  if (!fallback) return;
  removeWindowPart("project-fallback");
  fallback = null;
}

/// The project's custom agents: the named agents its folder defines for
/// each harness on one machine, and the user's own there.
const cags = makeCustomAgents({ redraw: () => redrawProjects(), start: startWithAgent });

/// "Custom agents", read on this Mac's checkout or else the first machine
/// with one. Named apart from "Agents", the ones running now.
/// How many custom agents the project has, as far as read.
export function customAgentsCount(rows: WhereRow[]) { return cags.count(agentsPlace(rows)); }
export function customAgentsSection(rows: WhereRow[]) { return cags.section(agentsPlace(rows)); }
export function disposeCustomAgents() { cags.dispose(); }

/// Start on a custom agent: the bar on the machine it was read on, with
/// its harness picked and "Use the <name> agent to " to finish.
function startWithAgent(a: CustomAgentRow, place: AgentsPlace) {
  const name = state.picked;
  if (!name) return;
  if (pc.project !== name) drawProjectComp(name);
  pc.machine = place.id;
  pc.picked.agent = a.harness;
  fillBar(name, agentPrompt(a.name), null);
}

let whereView: { project: string; host: HTMLElement } | null = null;

/// Each machine's checkout, sync state and terminals in one flat section.
export function whereSection(p: Project, rows: WhereRow[], known: boolean) {
  if (terms.list(p.name) === undefined) void terms.load(p.name);
  terms.follow(p.name);
  if (p.imported) fsync.watch(p.name);
  const model = p.imported ? fsync.page(p.name) : null;
  const all = terms.list(p.name);
  const items: ProjectMachineItem[] = [];

  for (const m of rows) {
    const host = state.home?.machines?.find(x => x.id === m.id);
    if (host?.can_host === false && !m.folder) continue;
    const detail = model?.detail?.machines.find(s => s.machine_id === m.id);
    const online = host?.online ?? (detail ? detail.status !== "offline" : m.kind !== "offline");
    const change = m.folder && p.imported && online ? () => changeFolder(p.name, m, m.folder ?? "") : null;
    const remove = m.folder && p.imported ? { why: blocked(p.name, rows, m), run: () => confirmRemove(p.name, m) } : null;
    const missing = m.folder ? null : missingMachine(p, m), git = m.folder ? gitControls(p, m, online) : null;
    const menu = m.folder ? (event: MouseEvent) => {
      const from = m.folder;
      if (!from) return;
      showMachineMenu({ id: m.id, name: m.name, online, can_host: host?.can_host }, () => [
        ...checkoutItems(p, m, from), ...(p.imported ? [...changeFolderItems(p.name, m), ...removeMenuItems(p.name, rows, m)] : []),
      ], event);
    } : null;
    const terminals = m.folder ? all?.filter(t => t.machine_id === m.id) ?? [] : [];
    items.push({ id: m.id, name: m.name, online, folder: m.folder ? plainPath(m.folder) : null, change, remove, missing, git, menu,
      terminals: terminals.map(t => ({ id: t.id, name: t.name, title: `${t.machine_name}: ${t.command} in ${t.cwd}`, sub: `${t.machine_name} · ${terminalWhere(t)}`, note: terminalNote(t, ms => formatAgo(ms, "short")), running: t.state === "running", open: () => { go("pane", { draft: { project: p.name, machine: t.machine_id, terminal: t } }); } })),
      terminalSummary: all === undefined ? "Reading…" : all === null ? "Unavailable" : !online ? "Machine offline" : terminals.length ? plural(terminals.length, "terminal", "terminals") : "None running" });
  }
  if (!whereView || whereView.project !== p.name) {
    if (whereView) removeWindowPart("project-where");
    const host = h("section", "pm-section");
    whereView = { project: p.name, host };
  }
  const view = whereView, note = pageFresh("where", p.name, readingWhere.has(p.name));
  showWindowParts([{ part: "project-where", host: view.host, identity: p.name, content: createElement(MachinesSection, { sync: fsync, project: p.name, items, model, imported: p.imported, known,
    count: whereCount(rows, false), fresh: note }) }]);
  return view.host;
}

/// The coding agents running in the project's folders now.
let runningView: { host: HTMLElement } | null = null;

export function disposeRunningAgents() { if (runningView) removeWindowPart("project-running"); runningView = null; }

export function agentsSection(running: RunningAgent[]) {
  if (!runningView) {
    const host = h("div");
    host.id = "sec-project-agents";
    runningView = { host };
  }
  const current = runningView;
  showWindowParts([{ part: "project-running", host: current.host, content: createElement(ProjectRunning, { running }) }]);
  return current.host;
}
