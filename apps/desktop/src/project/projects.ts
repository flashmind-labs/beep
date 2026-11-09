import { MV } from "../agents/move-store.ts";
// The Projects page's overview: the list, groups and cards of every
// project, and a picked project's detail sections.
import { sessionVisible } from "../agents/closing.ts";
import { ago, plural, since } from "../ui/format.ts";
import { dbtn as button, emptyPanel, errorNote, freshNote, svgEl as icon } from "../ui/parts.ts";
import { CI_TITLES, ciState, filterByName, listGroups, machineChips, projectBranch, splitName } from "./projectlist-state.ts";
import type { GitRow } from "./gitsync-state.ts";
import type { RunsRow } from "./runs.ts";
import type { MachineDetail, SyncDetail } from "./filesync.ts";
import { pageAgent } from "./agent.ts";
import { invoke } from "../ui/tauri.ts";
import { awaitsView, projectRunning, whereRows, whereCount, overviewGroups, barSegments, countsLine, latestLine, syncedBy, workersLabel, type OverviewCard, type OverviewCardRow, type OverviewGroup, type Project, type ProjectGroup, plainPath, type ProjectMachineRow, type WhereRow } from "./projects-state.ts";
import { el as h } from "../ui/dom.ts";
import { makeCustomAgents } from "./customagents.ts";
import { type AgentsPlace, type CustomAgentRow, agentPrompt, agentsPlace } from "./customagents-state.ts";
import { pages, state, warm, pageFresh } from "../center/state.ts";
import { projectDot, section, plusLabel } from "../ui/pieces.ts";
import { machineMenuWith } from "../machines/table.ts";
import { removeButton, removeMenuItems } from "./removemachine.ts";
import { changeFolder, changeFolderItems } from "./changefolder.ts";
import { runningRow } from "../center/home.ts";
import { single } from "../center/openitems.ts";
import { setProjectOnTop, newWindowOn, openProjects, addProject } from "../center/sidebar.ts";
import { groupMenu, editGroup, startRename, renameGroup, removeGroup } from "./groups.ts";
import { splitOn, projectSplit, panelSlot } from "./split.ts";
import { redrawProjects, redrawProjectsPage, pc, terms, fsync } from "./sections.ts";
import { drawProjectComp, fillBar } from "./composer.ts";
import { whereRow, gitCells, checkoutItems, readingWhere, whereDue, loadWhere, gitDue, loadGit, sessionsSection } from "./loads.ts";
import { terminalLine } from "../terminals/view.ts";
import { go } from "../center/views.ts";
import { IC } from "../ui/icons.ts";
import { lazyRows } from "../ui/lazyrows.ts";
import { byRecent, type RunningAgent } from "../agents/running-state.ts";

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
export function overviewSection(rows: Project[]) {
  const groups = overviewGroups(rows, state.overview, state.top, state.groups);
  // One org at a time; the rest draw as the page scrolls toward them.
  const out = lazyRows("projects-overview", "", groups.length, (from, to) => groups.slice(from, to).map(g => g.group ? groupSection(g.group, g)
    : h("section", "pj-org",
      h("h3", "", g.org ? `${g.org}/` : "Other folders", h("small", "", plural(g.cards.length, "repo", "repos"))),
      h("div", "pj-grid", ...g.cards.map(projectCard)))), 0, 1);
  const add = h("div", "cc-panel cc-row pj-add",
    h("span", "t", h("b", "", "Add a project"), h("span", "", "A folder or a repository to clone.")),
    button("Add", "", addProject));
  return [...out, add];
}

/// A group's section of the overview: its name (or the field renaming it),
/// how many repos, its folders and its buttons, then its cards.
function groupSection(g: ProjectGroup, section: OverviewGroup<Project, OverviewCard>) {
  const n = section.cards.length;
  const head = h("h3", "gp", icon(IC.folder));
  const editing = state.renaming?.id === g.id ? state.renaming : null;

  if (editing) {
    // What is typed lives in `state.renaming`, so a refresh that draws the
    // page again keeps it.
    const input = h("input");
    input.value = editing.draft;
    input.setAttribute("aria-label", "Group name");
    input.addEventListener("input", () => { editing.draft = input.value; });
    const finish = (save: boolean) => {
      if (state.renaming !== editing) return;

      if (save) renameGroup(g, input.value);
      else { state.renaming = null; redrawProjects(); }
    };
    input.addEventListener("keydown", e => {
      if (e.key === "Enter") { e.preventDefault(); finish(true); }
      else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); finish(false); }
    });
    head.append(input, h("span", "gp-acts", button("Save", "pri", () => finish(true)), button("Cancel", "quiet", () => finish(false))));
    // Focused once it is on the page.
    requestAnimationFrame(() => {
      if (!input.isConnected || document.activeElement === input) return;
      input.focus();
      input.setSelectionRange(input.value.length, input.value.length);
    });
  } else {
    const paths = h("span", "gp-paths", g.paths.map(p => (p.startsWith("/") ? p : `~/${p}`)).join(", "));
    paths.title = "Matched on every machine";
    head.append(g.name, h("small", "", plural(n, "repo", "repos")), paths,
      h("span", "gp-acts",
        button("Rename", "quiet", () => startRename(g)),
        button("Folders…", "quiet", () => editGroup(g)),
        button("Remove", "quiet warn", () => removeGroup(g))));
    head.addEventListener("contextmenu", e => groupMenu(e, g));
  }

  const body = n ? h("div", "pj-grid", ...section.cards.map(projectCard)) : h("span", "gp-none", "No projects in these folders yet.");
  return h("section", "pj-org", head, body);
}

function projectCard(c: OverviewCardRow<Project, OverviewCard>) {
  const pin = h("button", `pa${c.pinned ? " on" : ""}`, icon(IC.pin));
  pin.type = "button";
  pin.setAttribute("aria-label", c.pinned ? "Unpin" : "Pin to top");
  pin.addEventListener("click", e => { e.stopPropagation(); setProjectOnTop(c.name, !c.pinned); });
  const repo = h("b", "", c.repo);
  repo.title = c.name;
  const card = h("div", "pj-card", h("div", "h", projectDot(c.i), repo, pin, h("small", "", c.sum ? workersLabel(c.sum.workers) : (c.agents ? plural(c.agents, "agent", "agents") : "idle"))));
  card.tabIndex = 0;
  card.setAttribute("role", "button");
  card.title = c.name;

  if (c.sum) {
    const bar = h("div", "pj-bar");

    for (const s of barSegments(c.sum.tasks)) {
      const seg = h("i", s.state);
      seg.style.width = `${s.share}%`;
      seg.title = `${s.n} ${s.state}`;
      bar.append(seg);
    }

    const [open, ...rest] = countsLine(c.sum.tasks);
    card.append(bar, h("span", "line", h("b", "", open), rest.map(x => ` · ${x}`).join("")));
    const latest = c.sum.latest;
    card.append(h("span", "line", latestLine(latest), latest ? h("small", "", ` · ${ago(latest.updated_at)}`) : null));
  } else {
    card.append(h("span", "line", c.last ? `Last activity ${since(c.last)}` : "No recent activity"));
  }

  if (c.machines.length) card.append(h("span", "line sync", `on ${c.machines.map(m => m.name).join(", ")}`));
  // Only a project's page reads what Syncthing keeps, so that is what shows
  // here: no read per card.
  const sync = syncedBy(c.name, state.sync, state.where.get(c.name) ?? pages.get<ProjectMachineRow[]>("where", c.name));
  if (sync) card.append(h("span", "line sync", sync));
  const open = () => openProjects(c.name);
  newWindowOn(card, card, c.name);
  card.addEventListener("click", open);
  card.addEventListener("keydown", e => { if (e.target === card && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); open(); } });
  return card;
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

/// Each machine's checkout, sync state and terminals in one flat section.
export function whereSection(p: Project, rows: WhereRow[], known: boolean) {
  if (terms.list(p.name) === undefined) void terms.load(p.name);
  terms.follow(p.name);

  const machines = (sync: MachineDetail[], syncRow: (m: MachineDetail) => HTMLElement) => {
    const all = terms.list(p.name);
    const list = h("div", "pm-list");

    for (const m of rows) {
      const host = state.home?.machines?.find(x => x.id === m.id);
      if (host?.can_host === false && !m.folder) continue;

      const detail = sync.find(s => s.machine_id === m.id);
      const online = host?.online ?? (detail ? detail.status !== "offline" : m.kind !== "offline");
      const actions = h("span", "pm-actions");
      const head = h("div", "pm-head", h("span", `ddot${online ? "" : " off"}`), h("b", "", m.name));
      const machine = h("article", "pm-machine", head);
      machine.setAttribute("aria-label", m.name);

      if (m.folder) {
        const from = m.folder;
        const folder = h("code", "pm-folder", plainPath(from));
        folder.title = plainPath(from);
        head.append(folder);

        if (p.imported && online) {
          const change = button("Folder…", "small", () => changeFolder(p.name, m, from));
          change.setAttribute("aria-label", `Change folder on ${m.name}`);
          actions.append(change);
        }

        if (p.imported) actions.append(removeButton(p.name, rows, m));

        const git = gitCells(p, m);
        const status = detail ? syncRow(detail) : h("div", "fs-row pm-sync", h("div", "fs-m", online ? (fsync.detail(p.name) === null ? "Sync details unavailable" : "Reading sync state…") : "Machine offline"));
        if (!online) git.buttons.forEach(b => { if (b instanceof HTMLButtonElement) b.disabled = true; });

        const syncHead = status.querySelector(".fs-h");
        const gitDetail = h("span", "pm-git", git.note, h("span", "pm-actions", ...git.buttons));
        if (git.note || git.buttons.length) (syncHead ?? status).append(gitDetail);

        const terminals = all?.filter(t => t.machine_id === m.id) ?? [];
        const terminalHead = h("div", "pm-terminal-head", h("span", "", "Terminals"), h("small", "", all === undefined ? "Reading…" : all === null ? "Unavailable" : !online ? "Machine offline" : terminals.length ? plural(terminals.length, "terminal", "terminals") : "None running"));
        machine.append(h("div", "pm-body", status, h("hr"), terminalHead,
          ...terminals.map(t => terminalLine(t, () => go("pane", { draft: { project: p.name, machine: t.machine_id, terminal: t } })))));
        machineMenuWith({ id: m.id, name: m.name, online, can_host: host?.can_host }, () => [
          ...checkoutItems(p, m, from),
          ...(p.imported ? [...changeFolderItems(p.name, m), ...removeMenuItems(p.name, rows, m)] : []),
        ], head);
      } else {
        const cells = whereRow(p, m);
        head.append(...cells.slice(1, 2));
        if (cells[2]) actions.append(cells[2]);
      }

      head.append(actions);
      list.append(machine);
    }

    if (!list.childElementCount) list.append(emptyPanel(h("span", "", "No machines have this project.")));
    return list;
  };

  const contents = p.imported ? fsync.panel(p.name, undefined, machines) : [machines([], () => h("div"))];
  return h("section", "pm-section", ...contents,
    known ? null : h("small", "", `on ${whereCount(rows, false)}`), freshNote(pageFresh("where", p.name, readingWhere.has(p.name))));
}

/// The coding agents running in the project's folders now.
export function agentsSection(running: RunningAgent[]) {
  const list = running.length
    ? h("div", "cc-panel", ...byRecent(running).map(runningRow))
    : emptyPanel(h("span", "", "No agent is working in this project right now."));
  return section("sec-project-agents", "Agents", running.length || "", null, list);
}
