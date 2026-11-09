import { createElement } from "react";
import { removeWindowPart, removeWindowParts, showWindowParts } from "../center/window-root.ts";
import { ProjectRunning } from "./running-view.tsx";
import { OverviewGroupView } from "./overview-view.tsx";
import { MV } from "../agents/move-store.ts";
// The Projects page's overview: the list, groups and cards of every
// project, and a picked project's detail sections.
import { sessionVisible } from "../agents/closing.ts";
import { ago, plural } from "../ui/format.ts";
import { dbtn as button, emptyPanel, errorNote } from "../ui/parts.ts";
import { CI_TITLES, ciState, filterByName, listGroups, machineChips, projectBranch, splitName } from "./projectlist-state.ts";
import type { GitRow } from "./gitsync-state.ts";
import type { RunsRow } from "./runs.ts";
import type { SyncDetail } from "./filesync.ts";
import { pageAgent } from "./agent.ts";
import { invoke } from "../ui/tauri.ts";
import { awaitsView, projectRunning, whereRows, whereCount, overviewGroups, syncedBy, type OverviewCard, type Project, plainPath, type ProjectMachineRow, type WhereRow } from "./projects-state.ts";
import { el as h } from "../ui/dom.ts";
import { makeCustomAgents } from "./customagents.ts";
import { type AgentsPlace, type CustomAgentRow, agentPrompt, agentsPlace } from "./customagents-state.ts";
import { pages, state, warm, pageFresh } from "../center/state.ts";
import { plusLabel } from "../ui/pieces.ts";
import { showMachineMenu } from "../machines/table.ts";
import { MachinesSection, type ProjectMachineItem } from "./machines-section-view.tsx";
import { blocked, confirmRemove, removeMenuItems } from "./removemachine.ts";
import { changeFolder, changeFolderItems } from "./changefolder.ts";
import { single } from "../center/openitems.ts";
import { newWindowOn, openProjects, addProject } from "../center/sidebar.ts";
import { splitOn, projectSplit, panelSlot, projectPanel } from "./split.ts";
import { redrawProjects, redrawProjectsPage, pc, terms, fsync } from "./sections.ts";
import { drawProjectComp, fillBar } from "./composer.ts";
import { missingMachine, gitControls, checkoutItems, readingWhere, whereDue, loadWhere, gitDue, loadGit, sessionsSection } from "./loads.ts";
import { terminalNote, terminalWhere } from "../terminals/terminals.ts";
import { go } from "../center/views.ts";
import { lazyRows } from "../ui/lazyrows.ts";
import { type RunningAgent } from "../agents/running-state.ts";

export const importButton = (cls: string) => plusLabel("Add a project", cls, addProject);

/// What the Projects list is filtered by; one field kept across draws, so
/// typing keeps its focus.
const filter = h("input", "pl-filter");
filter.placeholder = "Filter projects";
filter.setAttribute("aria-label", "Filter projects");
filter.addEventListener("input", () => redrawProjects());

/// The filter field, refocused after a draw that moved it while it had focus.
export function projectFilter() {
  if (document.activeElement === filter) queueMicrotask(() => filter.focus());
  return filter;
}

/// One row per project, "Working now" above "Others": its name, branch,
/// a chip per machine with its sync, agents running, CI and last activity.
/// Only what is read already shows; the list reads nothing itself.
export function projectList(rows: Project[]) {
  const online = new Map((state.home?.machines || []).map(m => [m.id, m.online]));
  const head = h("div", "pl-row pl-head", ...["Project", "Branch", "Machines", "Agents", "CI", "Activity"].map(t => h("span", "", t)));
  const list = h("div", "pl", head);
  const shown = filterByName(rows, filter.value);
  for (const g of listGroups(shown)) list.append(h("div", "pl-grp", g.label), ...g.rows.map(p => projectListRow(p, online)));
  if (!shown.length) list.append(h("div", "pl-none", "No project matches."));
  return list;
}

function projectListRow(p: Project, online: ReadonlyMap<string, boolean>) {
  const { owner, repo } = splitName(p.name);
  const branch = projectBranch(state.gitsync.get(p.name) ?? pages.get<GitRow[]>("gitsync", p.name), state.mac?.id || null);
  const runs = pages.get<RunsRow | null>("runs", pageAgent(state.agents, p.name, state.subscribed)?.project_id ?? "");
  const ci = ciState(runs?.runs, branch);
  const chips = machineChips(p, { online, sync: state.sync, detail: pages.get<SyncDetail | null>("filesync", p.name) }).map(c => {
    const chip = h("span", "pl-chip", h("i", `pl-dot ${c.state}`), c.name);
    chip.title = c.title;
    return chip;
  });
  const dot = h("span", `pl-ci ${ci}`);
  dot.title = CI_TITLES[ci];
  const name = h("span", "pl-name", h("span", "pl-owner", owner), h("b", "", repo));
  name.title = p.url || p.name;
  const row = h("button", "pl-row",
    name,
    h("span", "pl-branch", branch || ""),
    h("span", "pl-mach", ...chips),
    h("span", "pl-ag", ...(p.agents ? [h("i", "pl-dot live"), String(p.agents)] : [])),
    dot,
    h("span", "pl-last", p.last ? ago(p.last) : ""));
  row.type = "button";
  row.id = `p-${p.name}`;
  row.addEventListener("click", () => openProjects(p.name));
  newWindowOn(row, row, p.name);
  return row;
}

/// Every project as a card, grouped by org: its tasks per state, its
/// workers, what changed last and where it lives.
const overviewRoots = new Map<string, { host: HTMLElement }>();

export function disposeOverview() {
  removeWindowParts([...overviewRoots.keys()].map(id => `project-overview:${id}` as const));
  overviewRoots.clear();
}

export function overviewSection(rows: Project[]) {
  const groups = overviewGroups(rows, state.overview, state.top, state.groups);
  const key = (g: typeof groups[number]) => g.group ? `group:${g.group.id}` : `org:${g.org ?? ""}`;
  const valid = new Set(groups.map(key));
  const removed = [...overviewRoots.keys()].filter(id => !valid.has(id));
  removeWindowParts(removed.map(id => `project-overview:${id}` as const));
  for (const id of removed) overviewRoots.delete(id);
  let drawing = true;
  const out = lazyRows("projects-overview", "", groups.length, (from, to) => {
    const visible = groups.slice(from, to).map(group => {
      const id = key(group);
      let entry = overviewRoots.get(id);
      if (!entry) {
        const host = h("section", "pj-org");
        entry = { host };
        overviewRoots.set(id, entry);
      }
      return { id, group, host: entry.host };
    });
    if (visible.length) showWindowParts(visible.map(({ id, group, host }) => ({ part: `project-overview:${id}` as const, host,
      content: createElement(OverviewGroupView, { group, sync: (name: string) => syncedBy(name, state.sync, state.where.get(name) ?? pages.get<ProjectMachineRow[]>("where", name)) }) })));
    return visible.map(({ host }) => drawing ? panelSlot(host) : host);
  }, 0, 1, row => projectPanel(row) ?? row);
  drawing = false;
  const add = h("div", "cc-panel cc-row pj-add", h("span", "t", h("b", "", "Add a project"), h("span", "", "A folder or a repository to clone.")), button("Add", "", addProject));
  return [...out, add];
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
export function projectDetail(p: Project) {
  if (p.imported) warm(p.name);
  const machines = p.imported ? state.where.get(p.name) : null;
  if (p.imported && (machines === undefined || whereDue.has(p.name))) loadWhere(p.name);
  if (p.imported && (!state.gitsync.has(p.name) || gitDue.has(p.name))) loadGit(p.name);
  const rows = whereRows(p, machines || null, state.mac?.id || null);
  const running = projectRunning(p, state.home?.running, state.sessions.get(p.name), [...MV.records.values()]).filter(sessionVisible);
  if (splitOn(p)) return projectSplit(p, rows, !!machines, running);

  // The project agent's view is on its way, or who has an agent is not known yet.
  if (awaitsView(p, state.agents)) {
    const error = state.pview.get(p.name)?.error;
    return [error ? errorNote(`Could not read the project: ${error}`) : emptyPanel(h("span", "", "Reading the project…"))];
  }

  // Without a project agent there are no tasks, runs or code host lists.
  return [h("div", "pj-cols pj-half", sessionsSection(p, running), h("div", "ca-col", whereSection(p, rows, !!machines), agentsSection(running), customAgentsSection(rows)))];
}

/// The project's custom agents: the named agents its folder defines for
/// each harness on one machine, and the user's own there.
const cags = makeCustomAgents({ redraw: () => redrawProjects(), start: startWithAgent, slot: panelSlot });

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
      terminals: terminals.map(t => ({ id: t.id, name: t.name, title: `${t.machine_name}: ${t.command} in ${t.cwd}`, sub: `${t.machine_name} · ${terminalWhere(t)}`, note: terminalNote(t, ago), running: t.state === "running", open: () => { go("pane", { draft: { project: p.name, machine: t.machine_id, terminal: t } }); } })),
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
  return panelSlot(view.host);
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
  return panelSlot(current.host);
}
