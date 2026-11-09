import { MV } from "../agents/move-store.ts";
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { ProjectNavigation, ProjectStatus } from "./split-view.tsx";
import { ProjectTasks } from "./tasks-view.tsx";
import { ProjectIssues } from "./issues-view.tsx";
// A project page's split: the list of tasks, PRs, issues and machines on
// the left, the picked row's view on the right.
import { sessionVisible } from "../agents/closing.ts";
import { harnessEl } from "../ui/harnessicon.ts";
import { ago, plural } from "../ui/format.ts";
import { dbtn as button, emptyPanel, errorNote, stateChip } from "../ui/parts.ts";
import { projectPastSessions, type Project, type WhereRow } from "./projects-state.ts";
import { el as h, type Kid } from "../ui/dom.ts";
import { agentFor, pageAgent, cardsSection } from "./agent.ts";
import { terminalTile, type TerminalRow } from "../terminals/terminals.ts";
import { fileSyncTile } from "./filesync.ts";
import { taskTile, machineTile, askingSessions } from "./board.ts";
import { issueMeta, prChip, prMeta } from "./threads.ts";
import { arranged, issueSelection, listControls, setIssueSelection, startBar } from "./openlist.ts";
import type { StartEntry } from "./openlist-state.ts";
import { pages, state } from "../center/state.ts";
import { projectBranch } from "./projectlist-state.ts";
import type { GitRow } from "./gitsync-state.ts";
import { line, section } from "../ui/pieces.ts";
import { homeBox, runningStatus } from "../center/home.ts";
import { terminalLine } from "../terminals/view.ts";
import { go } from "../center/views.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { agentsSection, customAgentsCount, customAgentsSection, disposeCustomAgents, disposeOverview, whereSection } from "./projects.ts";
import { pa, terms, fsync, revs, gitHistory, thr, details, runs, projectsEl, redrawProjects, agentActs, repoActs, answerCard } from "./sections.ts";
import { sessionsSection, sessionsList, sessionsDue, loadSessions } from "./loads.ts";
import { byRecent, type RunningAgent, runningKey } from "../agents/running-state.ts";
import { runningLineAt } from "../agents/place.ts";

const panelSlots = new WeakMap<Element, HTMLElement>();

export function projectPanel(slot: Element): HTMLElement | null {
  return panelSlots.get(slot) ?? null;
}

export function panelSlot(panel: HTMLElement): HTMLElement {
  const slot = h("div");
  panelSlots.set(slot, panel);
  return slot;
}

interface ProjectShell {
  project: string;
  navigation: HTMLElement;
  navRoot: Root;
  strip: HTMLElement;
  stripRoot: Root;
  issues: HTMLElement;
  issuesRoot: Root;
  tasks: HTMLElement;
  tasksRoot: Root;
}
let shell: ProjectShell | null = null;

function projectShell(project: string) {
  if (shell?.project === project) return shell;
  if (shell) { shell.navRoot.unmount(); shell.stripRoot.unmount(); shell.issuesRoot.unmount(); shell.tasksRoot.unmount(); }
  const navigation = h("div", "pj-list");
  const strip = h("div", "cc-mstrip pj-strip");
  const issues = h("div", "pj-det");
  const tasks = h("div", "pj-cols");
  shell = { project, navigation, strip, issues, tasks, navRoot: createRoot(navigation), stripRoot: createRoot(strip), issuesRoot: createRoot(issues), tasksRoot: createRoot(tasks) };
  return shell;
}

function taskPanel(project: string, view: Parameters<typeof ProjectTasks>[0]["view"]) {
  const shell = projectShell(project);
  flushSync(() => shell.tasksRoot.render(createElement(ProjectTasks, { view, act: agentActs })));
  return panelSlot(shell.tasks);
}

/// The split shows for an imported project whose agent's view has loaded, and with no
/// subscription for every imported project: it has no agent to wait for.
export function splitOn(p: Project) {
  if (!p.imported) return false;
  return state.subscribed ? !!agentFor(state.agents, p.name) && !!state.pview.get(p.name)?.view : true;
}

/// Picks item `key` of the split's list, or the first one with null, and
/// stops the polling of every other item. With `draw` false the caller
/// draws.
export function openTile(key: string | null, draw = true) {
  if (!draw && key === null) {
    disposeCustomAgents();
    disposeOverview();
    if (shell) { shell.navRoot.unmount(); shell.stripRoot.unmount(); shell.issuesRoot.unmount(); shell.tasksRoot.unmount(); shell = null; }
  }
  pa.tile = key === "sync" || key === "terminals" ? "machines" : key;
  if (key !== "actions") runs.stop();
  if (pa.tile !== "machines") { fsync.stop(); terms.stop(); }
  if (key !== "review") revs.stop();
  if (key !== "branches") gitHistory.stop();
  if (!draw) return;
  redrawProjects();
  projectsEl().querySelector(":scope > .cc-view .pj-det")?.scrollTo({ top: 0 });
}

/// One row of the split's list: it shows its detail at the right, or with
/// `open` goes somewhere else (a running agent opens in the pane).
export interface SplitItem {
  key: string;
  lead?: Kid;
  title: string;
  sub: string;
  /// Labels, as chips after `sub`.
  tags?: string[];
  /// When it opened, in milliseconds since the Unix epoch; none when unknown.
  opened?: number | null;
  /// What Start N agents takes of an issue.
  start?: StartEntry;
  right?: Kid;
  open?: () => void;
  /// What right-clicking the row does.
  menu?: (e: MouseEvent) => void;
  detail?: () => Node[];
}

/// A list's label filter: the labels its items carry, the one picked, and
/// what picking one does.
export type LabelFilter = { all: string[]; on?: string; pick: (label: string) => void };
/// A group of the list: its title, rows, and a note in place of rows.
/// A group without a title is set off from the one above it by a rule.
export type SplitGroup = { title: string; items: SplitItem[]; note?: string; labels?: LabelFilter; controls?: HTMLElement };

/// The label each list is filtered to, by list key.
const listLabel = new Map<string, string>();

/// `items` filtered to the label picked for list `key`, and the chips that
/// pick one; `redraw` runs on a pick. A label gone from every item clears.
export function labelFilter<T>(key: string, items: T[], tags: (i: T) => string[] | undefined, redraw: () => void) {
  const all = [...new Set(items.flatMap(i => tags(i) || []))].sort();
  const pick = listLabel.get(key), on = pick && all.includes(pick) ? pick : undefined;
  const labels: LabelFilter = { all, on, pick: l => {
    if (on === l) listLabel.delete(key); else listLabel.set(key, l);
    redraw();
  } };
  return { items: on ? items.filter(i => tags(i)?.includes(on)) : items, labels };
}

/// A label chip: a click filters its list to it, a second click clears it.
export function labelChip(label: string, f: LabelFilter, tag: "button" | "i") {
  const c = h(tag, `pj-tag lab${label === f.on ? " on" : ""}`, label);
  if (c instanceof HTMLButtonElement) c.type = "button";
  c.addEventListener("click", e => { e.stopPropagation(); f.pick(label); });
  return c;
}

/// The note of a code host list: why it could not be read, or that it reads.
function hostNote(list: unknown[] | null | undefined, error: string, none: string) {
  if (error) return error;
  if (list === undefined) return "Reading…";
  if (list === null) return "The server needs an update before it lists these.";
  return list.length ? undefined : none;
}

/// The project page: everything going on in one list at the left, the
/// picked row's detail and actions at the right.
export function projectSplit(p: Project, rows: WhereRow[], known: boolean, running: RunningAgent[]): Node[] {
  // No view and no agent chat without a subscription: no card, tasks or plan.
  if (pa.tile === "sync" || pa.tile === "terminals") pa.tile = "machines";
  const view = state.pview.get(p.name)?.view ?? null;
  const agent = pageAgent(state.agents, p.name, state.subscribed)!;
  const pid = agent.project_id;
  const asked = pa.run;

  if (asked?.project === p.name) {
    pa.run = null;
    queueMicrotask(() => runs.show(pid, asked.id, asked.host));
  }

  const sessions = state.sessions.get(p.name);
  if (sessions === undefined || sessionsDue.has(p.name)) loadSessions(p.name);
  if (terms.list(p.name) === undefined) terms.load(p.name);
  revs.ensure(p.name);
  thr.ensure(pid);
  const acts = repoActs(p);
  const sync = state.sync.filter(s => s.project === p.name);
  const syncCount = fileSyncTile(fsync.forTile(p.name), sync, new Map(rows.map(m => [m.id, m.name]))).count;
  const cards = view && cardsSection(view, { ...agentActs, answer: (c, decision, said) => answerCard(agent, c, decision, said) });
  const groups: SplitGroup[] = [];
  // Overview, Needs you and Running head the list, each one line.
  const top: SplitItem[] = [];
  if (cards) top.push({ key: "needs", title: "Needs you", sub: String(view.cards.length), detail: () => [cards] });
  const project: SplitItem[] = [
    { key: "actions", title: "Actions", sub: runs.tile(pid).count || "", detail: () => [runs.section(agent)] },
    // A machine's terminals sit below it, as on Home.
    { key: "machines", title: "Machines", sub: [syncCount || machineTile(rows, sync).count, terminalTile(terms.list(p.name), ago).count].filter(Boolean).join(" · "), detail: () => [whereSection(p, rows, known)] },
  ];
  const threads = thr.threads(pid);

  if (threads?.length) {
    const waiting = threads.filter(t => t.needs_reply).length;
    project.push({ key: "threads", title: "Your pull requests and issues", sub: waiting ? `${waiting} ${waiting === 1 ? "needs" : "need"} a reply` : `${threads.length} open`, detail: () => thr.threadsPanel(agent) });
  }

  const runItems: SplitItem[] = byRecent(running).map(r => ({
    key: `run:${runningKey(r)}`, lead: harnessEl(r.agent), ...runningLineAt(r, state.home?.projects || []),
    right: runningStatus(r), open: () => go("pane", { agent: r }),
    menu: e => void sessionMenu(e, r, s => go("pane", { agent: s })),
  }));
  if (runItems.length) top.push({ key: "running", title: "Running", sub: String(runItems.length), detail: () => [agentsSection(running)] });
  groups.push({ title: "", items: project });
  const past = sessions ? projectPastSessions(sessions, running, [...MV.records.values()]).filter(sessionVisible) : [];
  groups.push({ title: "", items: [
    ...(view ? [{ key: "tasks", title: "Tasks", sub: taskTile(view.tasks, view.workers, askingSessions(running, state.home?.needs_you)).count || "None open", detail: () => [taskPanel(p.name, view)] }] : []),
    // Running agents have their own rows above; this keeps past sessions and the custom agents.
    { key: "agents", title: "Sessions", sub: past.length ? `${past.length} past` : "None yet", detail: () => sessionTabs(p, running, rows) },
    { key: "branches", title: "Git", sub: "History and worktrees", detail: () => [] },
  ] });
  const num = (n: number) => h("span", "num", `#${n}`);
  const prs = thr.prs(pid), issues = thr.issues(pid);
  const prItems: SplitItem[] = thr.prLines(agent, acts).map(l => {
    const chip = prChip(l), pr = l.pr;
    return {
      key: `pr:${pr.number}`, opened: pr.created_at, lead: num(pr.number), title: pr.title, sub: prMeta(l, ago).join(" · "), tags: pr.labels,
      right: chip && stateChip(chip.cls, chip.text, chip.cls === "running"),
      menu: e => thr.prMenu(e, agent, acts, l),
      detail: () => details.pane(pid, { ...pr, author: pr.mine ? "you" : pr.author, opened_at: pr.created_at }, [thr.prButtons(agent, acts, l)]),
    };
  });
  const prKey = `${p.name}:prs`;
  const prFilter = labelFilter(prKey, prItems, i => i.tags, redrawProjects);
  groups.push({ title: "Pull requests", note: hostNote(prs, thr.prsError(pid), "No open pull requests."), controls: listControls(prKey, redrawProjects, false), labels: prFilter.labels, items: arranged(prKey, prFilter.items, i => i.opened) });
  const issueItems: SplitItem[] = thr.issueLines(agent, acts).map(l => ({
    key: `issue:${l.number}`, opened: l.issue?.opened_at, start: l.issue ? { key: `issue:${l.number}`, project: p.name, number: l.number, title: l.title, url: l.url } : undefined,
    lead: num(l.number), title: l.title,
    sub: l.issue ? issueMeta(l.issue, ago) : `task: ${l.task!.title}`, tags: l.issue?.labels,
    right: l.task && stateChip(l.task.state, l.task.state, l.task.state === "running"),
    menu: l.issue ? (e => thr.issueMenu(e, agent, acts, l.issue!)) : undefined,
    detail: () => details.pane(pid, { number: l.number, title: l.title, url: l.url, author: l.issue?.author || "", opened_at: l.issue?.opened_at || 0 },
      [l.issue ? thr.issueButtons(agent, acts, l.issue) : button("Tasks", "", () => openTile("tasks"))]),
  }));
  const issueKey = `${p.name}:issues`;
  const issueFilter = labelFilter(issueKey, issueItems, i => i.tags, redrawProjects);
  const issueShown = arranged(issueKey, issueFilter.items, i => i.opened);
  const issueNote = hostNote(issues, thr.issuesError(pid), "No open issues.");
  // Too many to list at the left: the row opens them at the right.
  project.push({ key: "issues", title: "Issues", sub: issueNote || `${issueItems.length} open`, detail: () => [] });
  const overview = projectOverview(p, running, { run: runItems, prs: prItems, issues: issueItems, prNote: hostNote(prs, thr.prsError(pid), "No open pull requests."), issueNote }, machineTile(rows, sync));
  top.unshift({ key: "overview", title: "Overview", sub: "", detail: () => [overview.body] });
  groups.unshift({ title: "", items: top });
  // A review opens from Branches and an issue from Issues, each keeping its row lit.
  const all = groups.flatMap(g => g.items);
  const issue = issueItems.find(i => i.key === pa.tile);
  const picked = pa.tile === "review"
    ? { key: "branches", detail: () => revs.reviewPanel(p.name) }
    : issue ? { key: "issues", detail: () => [button("All issues", "quiet small pj-back", () => openTile("issues")), ...issue.detail!()] }
    : all.find(i => i.key === pa.tile && i.detail) || all[0];
  const currentShell = projectShell(p.name);
  flushSync(() => currentShell.navRoot.render(createElement(ProjectNavigation, { groups, picked: picked.key, openTile })));

  const error = state.pview.get(p.name)?.error;
  const history = pa.tile === "branches" ? gitHistory.panels(p.name) : null;
  const showIssues = pa.tile === "issues";
  const showReview = pa.tile === "review";
  if (showIssues) flushSync(() => currentShell.issuesRoot.render(createElement(ProjectIssues, {
    error: error ?? null, items: issueShown, filter: issueFilter.labels,
    controls: listControls(issueKey, redrawProjects, true),
    start: startBar(issueKey, issueShown.flatMap(item => item.start ?? []), redrawProjects),
    selection: (id: string) => issueSelection(issueKey, id),
    pick: (id: string, checked: boolean) => setIssueSelection(issueKey, id, checked, redrawProjects),
    openTile,
  })));
  const det = history?.detail || (showIssues ? currentShell.issues : showReview ? revs.reviewDetail(p.name, error ?? null) : h("div", "pj-det", error ? errorNote(`Could not refresh the project: ${error}`) : null, ...picked.detail!()));
  const waiting = threads?.filter(t => t.needs_reply).length ?? 0;
  const cells = [
    { label: projectBranch(state.gitsync.get(p.name) ?? pages.get<GitRow[]>("gitsync", p.name), state.mac?.id || null) || "", text: "branch", key: "branches" },
    { label: "Actions", text: runs.tile(pid).count || "", key: "actions", bad: runs.tile(pid).rows[0]?.chip?.text === "failed" },
    { label: plural(running.length, "agent", "agents"), text: "running", key: "running" },
    { label: "Machines", text: machineTile(rows, sync).count || "", key: "machines" },
    { label: plural(prItems.length, "PR", "PRs"), text: waiting ? `${waiting} ${waiting === 1 ? "needs" : "need"} a reply` : "open", key: "overview" },
    { label: plural(issueItems.length, "issue", "issues"), text: "open", key: "issues" },
  ];
  flushSync(() => currentShell.stripRoot.render(createElement(ProjectStatus, { cells, openTile })));
  return [panelSlot(currentShell.strip), h("div", `${history ? "pj-split gh-layout" : "pj-split"}${pa.tile ? " picked" : ""}`, panelSlot(history ? history.navigation : currentShell.navigation), history || showIssues || showReview ? panelSlot(det) : det)];
}

/// The Sessions row's tab on screen.
let sessionTab: "sessions" | "agents" = "sessions";

/// Past sessions and custom agents in two tabs, so each gets the whole height.
function sessionTabs(p: Project, running: RunningAgent[], rows: WhereRow[]): Node[] {
  const list = sessionsList(p, running, true);
  const bar = h("div", "cc-tabs pj-tabs");
  bar.setAttribute("role", "tablist");

  for (const [key, label, n] of [["sessions", "Sessions", list.count], ["agents", "Agents", customAgentsCount(rows)]] as const) {
    const b = h("button", "", label, n ? h("span", "n", String(n)) : null);
    b.type = "button";
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", String(key === sessionTab));
    b.addEventListener("click", () => { sessionTab = key; redrawProjects(); });
    bar.append(b);
  }

  return sessionTab === "sessions" ? [bar, list.body, ...(list.note ? [list.note] : [])] : [bar, customAgentsSection(rows)];
}

/// The agent view with terminal `t`, one of project `name`, in a pane tab with the keys.
function openTerminal(name: string, t: TerminalRow) {
  go("pane", { draft: { project: name, machine: t.machine_id, terminal: t } });
}

/// The project's Overview: what runs and ran on the left, the open work on
/// the right, each title opening that row of the list. Issues scroll in their box.
function projectOverview(p: Project, running: RunningAgent[], open: { run: SplitItem[]; prs: SplitItem[]; issues: SplitItem[]; prNote: Kid; issueNote: Kid }, where: ReturnType<typeof machineTile>) {
  const { run, prs, issues } = open;
  // A copy: the list beside it takes the lead node itself.
  const rowsOf = (items: SplitItem[]) => items.map(i => {
    const row = line(i.lead instanceof Node ? i.lead.cloneNode(true) : null, i.title, i.sub, [i.right, ...(i.tags || []).slice(0, 3).map(l => h("i", "pj-tag", l))], i.open || (() => openTile(i.key)));
    if (i.menu) row.addEventListener("contextmenu", i.menu);
    return row;
  });
  const box = (title: string, items: SplitItem[], key: string) => items.length ? homeBox("", title, items.length, () => openTile(key), rowsOf(items)) : null;
  const list = terms.list(p.name) || [];
  const machines = where.rows.map(r => {
    const row = line(h("span", `ddot${r.dot ? " off" : ""}`), r.text || "", r.note || "", [], () => openTile("machines"));
    row.title = r.title || r.text || "";
    return row;
  });
  const left = [
    box("Running", run, "running"),
    sessionsSection(p, running),
    machines.length ? homeBox("", "Machines", where.count, () => openTile("machines"), machines) : null,
    list.length ? homeBox("", "Terminals", list.length, () => openTile("machines"), list.map(t => terminalLine(t, () => openTerminal(p.name, t)))) : null,
  ];
  const right = [
    prs.length ? box("Pull requests", prs, prs[0].key) : section("", "Pull requests", "", null, emptyPanel(h("span", "", open.prNote))),
    issues.length
      ? section("", button("Issues", "lk", () => openTile("issues")), issues.length, null, h("div", "cc-panel pj-scroll", ...rowsOf(issues)))
      : section("", "Issues", "", null, emptyPanel(h("span", "", open.issueNote))),
  ];
  return { body: h("div", "pj-ov", h("div", "", ...left), h("div", "", ...right)) };
}
