// The Projects page's overview: the table, groups and cards of every
// project, and a picked project's detail sections.
import { ago, plural } from "../ui/format.ts";
import { dbtn as button, emptyPanel, errorNote, freshNote, svgEl as icon } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { byRecent, type RunningAgent } from "../center/home-state.ts";
import { awaitsView, whereRows, whereCount, overviewGroups, barSegments, countsLine, latestLine, syncedBy, workersLabel, type OverviewCard, type OverviewCardRow, type OverviewGroup, type Project, type ProjectGroup, type ProjectMachineRow, type WhereRow } from "./projects-state.ts";
import { el as h } from "../ui/dom.ts";
import { makeCustomAgents } from "./customagents.ts";
import { type AgentsPlace, type CustomAgentRow, agentPrompt, agentsPlace } from "./customagents-state.ts";
import { IC, pages, state, warm, pageFresh } from "../center/state.ts";
import { projectDot, section, plusLabel } from "../center/pieces.ts";
import { machineMenu } from "../machines/table.ts";
import { runningRow } from "../center/home.ts";
import { single } from "../center/openitems.ts";
import { setProjectOnTop, newWindowOn, openProjects, addProject } from "../center/sidebar.ts";
import { groupMenu, editGroup, startRename, renameGroup, removeGroup } from "./groups.ts";
import { splitOn, projectSplit } from "./split.ts";
import { redrawProjects, redrawProjectsPage, pc } from "./sections.ts";
import { drawProjectComp, fillBar } from "./composer.ts";
import { whereRow, readingWhere, whereDue, loadWhere, sessionsSection, hostOf } from "./loads.ts";

export const importButton = (cls: string) => plusLabel("Add a project", cls, addProject);

/// One project per row: its name and host, where it is on each machine,
/// the agents running in it and when one last did something.
export function projectTable(rows: Project[]) {
  const table = h("div", "cc-mt cc-pt", ...["Project", "Machine and folder", "Agents", "Last activity"].map(t => h("span", "th", t)));
  for (const p of rows) {
    const open = h("button", "", h("span", "", p.name));
    open.type = "button";
    open.title = p.url || p.name;
    open.addEventListener("click", () => openProjects(p.name));
    const name = h("span", "nm", h("b", "", open), h("span", "os", p.imported ? hostOf(p.url) : "From running agents"));
    name.id = `p-${p.name}`;
    newWindowOn(open, name, p.name);
    const where = h("span", "nm", ...p.machines.map(m => {
      const one = h("span", "os", h("b", "", m.name), ` ${m.folder}`);
      one.title = `${m.name}: ${m.folder}`;
      return one;
    }));
    if (!p.machines.length) where.append(h("span", "os", "–"));
    const last = h("span", "num", p.last ? ago(p.last) : "–");
    table.append(name, where, h("span", "num", p.agents ? String(p.agents) : "–"), last);
  }
  return table;
}

/// Every project as a card, grouped by org: its tasks per state, its
/// workers, what changed last and where it lives.
export function overviewSection(rows: Project[]) {
  const groups = overviewGroups(rows, state.overview, state.top, state.groups);
  const out = groups.map(g => g.group ? groupSection(g.group, g)
    : h("section", "pj-org",
      h("h3", "", g.org ? `${g.org}/` : "Other folders", h("small", "", plural(g.cards.length, "repo", "repos"))),
      h("div", "pj-grid", ...g.cards.map(projectCard))));
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
    card.append(h("span", "line", c.imported ? "No project agent yet" : "From running agents"));
  }
  // Only a project's page reads what Syncthing keeps, so that is what shows
  // here: no read per card.
  const sync = syncedBy(c.name, state.sync, state.where.get(c.name) ?? pages.get<ProjectMachineRow[]>("where", c.name));
  if (sync) card.append(h("span", "line sync", sync));
  const open = () => openProjects(c.name);
  newWindowOn(card, card, c.name);
  card.addEventListener("click", open);
  card.addEventListener("keydown", e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open(); } });
  return card;
}

/// The overview's cards; `null` from a server too old to sum projects up,
/// when the page keeps its table.
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
  const rows = whereRows(p, machines || null, state.mac?.id || null);
  const inside = (r: RunningAgent, m: { id: string; folder: string }) => r.machine_id === m.id && (r.cwd === m.folder || r.cwd?.startsWith(m.folder + "/"));
  const running = (state.home?.running || []).filter((r): r is RunningAgent => r.kind === "agent" && p.machines.some(m => inside(r, m)));
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
const cags = makeCustomAgents({ redraw: () => redrawProjects(), start: startWithAgent });

/// "Custom agents", read on this Mac's checkout or else the first machine
/// with one. Named apart from "Agents", the ones running now.
export function customAgentsSection(rows: WhereRow[]) {
  const place = agentsPlace(rows);
  const { body, count } = cags.view(place);
  return section("sec-custom-agents", "Custom agents", count || "", place ? button("Reload", "quiet small", () => cags.reload(place)) : null, body);
}

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

/// "Where it lives": every machine with the project's folder there, or why
/// there is none.
export function whereSection(p: Project, rows: WhereRow[], known: boolean) {
  const where = h("div", "cc-mt cc-pt pj-where", ...["Machine", "Folder", ""].map(t => h("span", "th", t)));
  where.style.gridTemplateColumns = "fit-content(40%) minmax(0,1fr) auto";
  for (const m of rows) {
    const cells = whereRow(p, m);
    where.append(...cells);
    if (m.id) machineMenu({ id: m.id, name: m.name, online: m.kind !== "offline" }, ...cells);
  }
  // Once every machine answered, the note says the count instead.
  return section("sec-where", "Where it lives", known ? "" : rows.filter(m => m.kind === "here").length, null, where,
    known && h("small", "", `on ${whereCount(rows, true)}`), freshNote(pageFresh("where", p.name, readingWhere.has(p.name))));
}

/// The coding agents running in the project's folders now.
export function agentsSection(running: RunningAgent[]) {
  const list = running.length
    ? h("div", "cc-panel", ...byRecent(running).map(runningRow))
    : emptyPanel(h("span", "", "No agent is working in this project right now."));
  return section("sec-project-agents", "Agents", running.length || "", null, list);
}
