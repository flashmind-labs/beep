import { lastProjectVisit, leaveProject, noteProjectVisit, readProjectSort, saveProjectSort } from "../../stores/project/project-preferences.ts";
import { projectChanged } from "../../stores/project/barstate.ts";
import { MACHINES } from "../../stores/machines/machine-store.ts";
import { removeWindowPart, showWindowParts } from "../../parts/center/window-root.tsx";
import { ProjectsOverview } from "./overview-view.tsx";
import { ProjectFallback } from "./split-view.tsx";
import { moveStore } from "../../stores/agents/move-store.ts";
// The Projects page's overview: the list, groups and cards of every
// project, and a picked project's detail sections.
import { sessionVisible } from "../../stores/agents/closing.ts";
import { projectRunning, whereRows, syncedBy, type Project } from "../../stores/project/projects-state.ts";
import { projectActivity, projectSortTime, overviewGroups, type ProjectSort } from "../../stores/project/overview-state.ts";
import { state, whereOf, peekSessions, sessionsOf, gitOf } from "../../stores/center/state.ts";
import { addProject } from "../../parts/project/addflow.ts";
import { splitOn, projectSplit } from "./split.tsx";
import { disposeProjectShell } from "./tiles.tsx";
import { hostPart, retainedHost, viewPart, type PagePart } from "./page-view.tsx";
import { agentsSection, customAgentsSection, whereSection } from "./detailsections.tsx";
import { whereDue, loadWhere, gitDue, loadGit, sessionsList } from "./loads.tsx";
import { OpenProjects } from "../center/home-overview.tsx";
import { homeForDisplay } from "../../stores/center/home-state.ts";

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

let projectSort = readProjectSort();

function changeSort(sort: ProjectSort) {
  projectSort = sort;
  ranked = null;
  saveProjectSort(sort);
  projectChanged.notify();
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
