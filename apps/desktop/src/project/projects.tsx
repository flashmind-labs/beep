import { lastProjectVisit, leaveProject, noteProjectVisit, readProjectSort, saveProjectSort } from "./project-preferences.ts";
import { MACHINES } from "../machines/machine-store.ts";
import { removeWindowPart, showWindowParts } from "../center/window-root.tsx";
import { ProjectRunning } from "./running-view.tsx";
import { ProjectsOverview } from "./overview-view.tsx";
import { ProjectFallback } from "./split-view.tsx";
import { moveStore } from "../agents/move-store.ts";
// The Projects page's overview: the list, groups and cards of every
// project, and a picked project's detail sections.
import { sessionVisible } from "../agents/closing.ts";
import { formatAgo, plural } from "../ui/format.ts";
import { projectRunning, whereRows, whereCount, syncedBy, type Project, type WhereRow } from "./projects-state.ts";
import { plainPath } from "./filesync.ts";
import { projectActivity, projectSortTime, overviewGroups, type ProjectSort } from "./overview-state.ts";
import { makeCustomAgents } from "./customagents.tsx";
import { type AgentsPlace, type CustomAgentRow, agentPrompt, agentsPlace } from "./customagents-state.ts";
import { state, whereOf, peekSessions, sessionsOf, gitOf, pageFresh } from "../center/state.ts";
import { showMachineMenu } from "../machines/table.ts";
import { MachinesSection, type ProjectMachineItem } from "./machines-section-view.tsx";
import { blocked, confirmRemove, removeMenuItems } from "./removemachine.ts";
import { changeFolder, changeFolderItems } from "./changefolder.ts";
import { addProject } from "../center/sidebar.tsx";
import { splitOn, projectSplit, disposeProjectShell } from "./split.tsx";
import { hostPart, retainedHost, viewPart, type PagePart } from "./page-view.tsx";
import { redrawProjects, pc, terms, fsync } from "./sections.ts";
import { drawProjectComp, fillBar } from "./composer.tsx";
import { missingMachine, gitControls, checkoutItems, readingWhere, whereDue, loadWhere, gitDue, loadGit, sessionsList } from "./loads.tsx";
import { terminalNote, terminalWhere } from "../terminals/terminals.tsx";
import { go } from "../center/views.tsx";
import { OpenProjects } from "../center/home-overview.tsx";
import { homeForDisplay } from "../center/home-state.ts";
import { type RunningAgent } from "../agents/running-state.ts";

/// Every project as a card, grouped by org: its agents, when it was last
/// active and where it lives. It stays in one host while the
/// Projects page is mounted, so opening a project and coming back keeps its groups drawn.
let overviewHost: HTMLElement | null = null;

export function disposeOverview() {
  leaveProject();
  if (!overviewHost) return;
  removeWindowPart("project-overview");
  overviewHost = null;
}

export function overviewSection(listed: Project[]): PagePart[] {
  leaveProject();
  // A card shows when something last happened, as Recent activity sorts it.
  const rows = listed.map(p => ({ ...p, last: projectActivity(p, peekSessions(p.name), null) || null }));
  const groups = overviewGroups(rows, state.top, state.groups, overviewOrder(rows));
  overviewHost ??= retainedHost(null);
  showWindowParts([{ part: "project-overview", host: overviewHost, content: <ProjectsOverview groups={groups} active={state.home ? <OpenProjects home={homeForDisplay(state.home)} /> : null} editing={state.renaming} sort={projectSort} changeSort={changeSort}
    sync={(name: string) => syncedBy(name, state.sync, whereOf(name))} add={addProject}
  /> }]);
  return [hostPart(overviewHost)];
}

let projectSort = readProjectSort("projects");

function changeSort(sort: ProjectSort) {
  projectSort = sort;
  ranked = null;
  saveProjectSort("projects", sort);
  redrawProjects();
}

/// The overview's recency, taken once per look at it: when projects come or
/// go, after a project was opened, and on a later draw while the pointer and
/// focus are elsewhere. Ambient updates do not move cards under the user. A
/// touch screen keeps `:hover` and focus on whatever was tapped last, so only
/// a pointer that hovers holds the order. A phone re-ranks once a minute.
let ranked: { names: string; at: number; activity: Map<string, number> } | null = null;
const RANK_MS = 60_000;
const HOVERS = matchMedia("(hover: hover)");

function overviewOrder(rows: readonly Project[]) {
  const names = rows.map(p => p.name).join("\n");
  const box = overviewHost;
  const busy = HOVERS.matches && !!box && (box.matches(":hover") || box.contains(document.activeElement));
  if (ranked && ranked.names === names && (busy || Date.now() - ranked.at < RANK_MS)) return ranked.activity;

  const activity = new Map(rows.map(p => [p.name, projectSortTime(projectSort, p, peekSessions(p.name) ?? null, lastProjectVisit(p.name))]));
  ranked = { names, at: Date.now(), activity };
  return activity;
}

/// One project: where it lives on every machine, with "Start an agent here"
/// per folder and "Copy here" where it is not, and the coding agents
/// running in it now.
export function projectDetail(p: Project): PagePart[] {
  noteProjectVisit(p.name);
  ranked = null;
  const machines = p.imported ? whereOf(p.name) : null;
  if (p.imported && (machines === undefined || whereDue.has(p.name))) loadWhere(p.name);
  if (p.imported && (gitOf(p.name) === undefined || gitDue.has(p.name))) loadGit(p.name);
  const rows = whereRows(p, machines || null, MACHINES.mac?.id || null);
  const running = projectRunning(p, state.home?.running, sessionsOf(p.name), [...moveStore.getState().records.values()]).filter(sessionVisible);
  if (splitOn(p)) return projectSplit(p, rows, !!machines, running);
  disposeProjectShell();

  // A project only agents ran in has no checkout to list runs or code host lists for.
  return [viewPart("fallback", <div key={p.name} className="pj-cols pj-half"><ProjectFallback
    session={sessionsList(p, running)}
    machines={whereSection(p, rows, !!machines)} running={agentsSection(running)} agents={customAgentsSection(rows)}
  /></div>)];
}

/// The project's custom agents: the named agents its folder defines for
/// each harness on one machine, and the user's own there.
const cags = makeCustomAgents({ redraw: () => redrawProjects(), start: startWithAgent, own: false });

/// "Custom agents", read on this Mac's checkout or else the first machine
/// with one. Named apart from "Agents", the ones running now.
/// How many custom agents the project has, as far as read.
export function customAgentsCount(rows: WhereRow[]) { return cags.count(agentsPlace(rows)); }
export function customAgentsSection(rows: WhereRow[]) { return cags.section(agentsPlace(rows)); }

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

/// Each machine's checkout, sync state and terminals in one flat section.
export function whereSection(p: Project, rows: WhereRow[], known: boolean) {
  if (terms.list(p.name) === undefined) void terms.load(p.name);
  terms.follow(p.name);
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
      terminalSummary: all === undefined ? "Reading…" : !online ? "Machine offline" : terminals.length ? plural(terminals.length, "terminal", "terminals") : "None running" });
  }
  const note = pageFresh("where", p.name, readingWhere.has(p.name));
  return <section key={p.name} className="pm-section"><MachinesSection sync={fsync} project={p.name} items={items} model={model} imported={p.imported} known={known}
    count={whereCount(rows, false)} fresh={note} /></section>;
}

/// The coding agents running in the project's folders now.
export function agentsSection(running: RunningAgent[]) {
  return <div id="sec-project-agents"><ProjectRunning running={running} /></div>;
}
