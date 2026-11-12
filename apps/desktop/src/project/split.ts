import { moveStore } from "../agents/move-store.ts";
import { createElement, Fragment, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { removeWindowPart, showWindowParts } from "../center/window-root.ts";
import { ProjectNavigation, ProjectOverview, ProjectStatus, RunningStatus, SessionTabs } from "./split-view.tsx";
import { ProjectBoard } from "./lanes.tsx";
import type { Lane } from "../center/lanes-state.ts";
import { frontLayout } from "../center/lanes-view.tsx";
import { ProjectTasks } from "./tasks-view.tsx";
import { ProjectIssues } from "./issues-view.tsx";
import { OpenListControls, OpenListStart } from "./openlist-view.tsx";
// A project page's split: the list of tasks, PRs, issues and machines on
// the left, the picked row's view on the right.
import { sessionVisible } from "../agents/closing.ts";
import { formatAgo, plural } from "../ui/format.ts";
import { projectPastSessions, type Project, type WhereRow } from "./projects-state.ts";
import { agentFor, pageAgent } from "./agent.ts";
import { FreshNote, NeedsYou, Parts, hostPart, retainedHost, threadsParts, viewPart, type PagePart } from "./page-view.tsx";
import { terminalTile, type TerminalRow } from "../terminals/terminals.ts";
import { fileSyncTile } from "./filesync.ts";
import { taskTile, machineTile, askingSessions } from "./board.ts";
import { issueMeta, prChip, prMeta } from "./threads.ts";
import { arranged, issueSelection, setIssueSelection } from "./openlist.ts";
import type { StartEntry } from "./openlist-state.ts";
import { pages, state } from "../center/state.ts";
import { projectBranch } from "./projectlist-state.ts";
import type { GitRow } from "./gitsync-state.ts";
import { boxOf, go } from "../center/views.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { agentsSection, customAgentsCount, customAgentsSection, whereSection } from "./projects.ts";
import { pa, terms, fsync, revs, gitHistory, thr, details, runs, redrawProjects, agentActs, repoActs, answerCard } from "./sections.ts";
import { sessionsHost, sessionsList, sessionsDue, loadSessions } from "./loads.ts";
import { byRecent, type RunningAgent, runningKey } from "../agents/running-state.ts";
import { runningLineAt } from "../agents/place.ts";
import { HarnessIcon } from "../ui/icons.tsx";
import { Chip, EmptyPanel, ErrorNote } from "../ui/kit.tsx";

/// The project's navigation and Overview stay mounted in their own elements while
/// other tiles show, so their rows, focus and scroll survive a visit to Git or Running.
interface ProjectShell {
  project: string;
  navigation: HTMLElement;
  overview: HTMLElement;
  navView: ReactNode;
  overviewView: ReactNode;
  overviewScroll: Map<string, number>;
}
let shell: ProjectShell | null = null;

function renderProjectShell(current: ProjectShell) {
  showWindowParts([{ part: "project-shell", host: null, identity: current.project, content: createElement(Fragment, null,
    createPortal(current.navView, current.navigation, "navigation"), createPortal(current.overviewView, current.overview, "overview")) }]);
}
export function disposeProjectShell() {
  if (!shell) return;
  removeWindowPart("project-shell");
  shell = null;
}
function projectShell(project: string) {
  if (shell?.project === project) return shell;
  disposeProjectShell();
  shell = { project, navigation: retainedHost("pj-list"), overview: retainedHost("pj-ov"), navView: null, overviewView: null, overviewScroll: new Map() };
  return shell;
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
  const current = shell;
  const scrolls = ["#sec-sessions .pj-scroll", ".pj-ov-issues"];
  if (draw && current?.overview.isConnected) for (const selector of scrolls) {
    const element = current.overview.querySelector<HTMLElement>(selector);
    if (element) current.overviewScroll.set(selector, element.scrollTop);
  }
  if (!draw && key === null) {
    disposeProjectShell();
  }
  pa.tile = key === "sync" || key === "terminals" ? "machines" : key;
  if (key !== "actions") runs.stop();
  if (pa.tile !== "machines") { fsync.stop(); terms.stop(); }
  if (key !== "review") revs.stop();
  if (key !== "branches") gitHistory.stop();
  if (!draw) return;
  redrawProjects();
  boxOf("projects")?.querySelector(":scope > .cc-view .pj-det")?.scrollTo({ top: 0 });
  if ((key === "overview" || key === null) && current) for (const selector of scrolls) {
    const element = current.overview.querySelector<HTMLElement>(selector), top = current.overviewScroll.get(selector);
    if (element && top !== undefined) element.scrollTop = top;
  }
}

/// One row of the split's list: it shows its detail at the right, or with
/// `open` goes somewhere else (a running agent opens in the pane).
export interface SplitItem {
  key: string;
  lead?: ReactNode;
  title: string;
  sub: string;
  /// Labels, as chips after `sub`.
  tags?: string[];
  /// When it opened, in milliseconds since the Unix epoch; none when unknown.
  opened?: number | null;
  /// What Start N agents takes of an issue.
  start?: StartEntry;
  right?: ReactNode;
  open?: () => void;
  /// What right-clicking the row does.
  menu?: (e: MouseEvent) => void;
  detail?: () => PagePart[];
}

/// A list's label filter: the labels its items carry, the one picked, and
/// what picking one does.
export type LabelFilter = { all: string[]; on?: string; pick: (label: string) => void };
/// A group of the list: its title, rows, and a note in place of rows.
/// A group without a title is set off from the one above it by a rule.
export type SplitGroup = { title: string; items: SplitItem[]; note?: string; labels?: LabelFilter; controls?: ReactNode };

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

/// The note of a code host list: why it could not be read, or that it reads.
function hostNote(list: unknown[] | null | undefined, error: string, none: string) {
  if (error) return error;
  if (list === undefined) return "Reading…";
  if (list === null) return "The server needs an update before it lists these.";
  return list.length ? undefined : none;
}

/// The project page: everything going on in one list at the left, the
/// picked row's detail and actions at the right.
export function projectSplit(p: Project, rows: WhereRow[], known: boolean, running: RunningAgent[]): PagePart[] {
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
  const cards = view?.cards.length ? viewPart("needs", createElement(NeedsYou, { cards: view.cards, act: { ...agentActs, answer: (c, decision, said) => answerCard(agent, c, decision, said) } })) : null;
  const groups: SplitGroup[] = [];
  // Overview, Needs you and Running head the list, each one line.
  const top: SplitItem[] = [];
  if (cards && view) top.push({ key: "needs", title: "Needs you", sub: String(view.cards.length), detail: () => [cards] });
  const project: SplitItem[] = [
    { key: "actions", title: "Actions", sub: runs.tile(pid).count || "", detail: () => [viewPart("actions", runs.section(agent))] },
    // A machine's terminals sit below it, as on Home.
    { key: "machines", title: "Machines", sub: [syncCount || machineTile(rows, sync).count, terminalTile(terms.list(p.name), ms => formatAgo(ms, "short")).count].filter(Boolean).join(" · "), detail: () => [viewPart("where", whereSection(p, rows, known))] },
  ];
  const threads = thr.threads(pid);

  if (threads?.length) {
    const waiting = threads.filter(t => t.needs_reply).length;
    project.push({ key: "threads", title: "Your pull requests and issues", sub: waiting ? `${waiting} ${waiting === 1 ? "needs" : "need"} a reply` : `${threads.length} open`, detail: () => threadsParts(thr.threadsPanel(agent)) });
  }

  const runItems: SplitItem[] = byRecent(running).map(r => ({
    key: `run:${runningKey(r)}`, lead: createElement(HarnessIcon, { agent: r.agent }), ...runningLineAt(r, state.home?.projects || []),
    right: createElement(RunningStatus, { row: r }), open: () => go("pane", { agent: r }),
    menu: e => void sessionMenu(e, r, s => go("pane", { agent: s })),
  }));
  if (runItems.length) top.push({ key: "running", title: "Running", sub: String(runItems.length), detail: () => [viewPart("running", agentsSection(running))] });
  groups.push({ title: "", items: project });
  const past = sessions ? projectPastSessions(sessions, running, [...moveStore.getState().records.values()]).filter(sessionVisible) : [];
  groups.push({ title: "", items: [
    ...(view ? [{ key: "tasks", title: "Tasks", sub: taskTile(view.tasks, view.workers, askingSessions(running, state.home?.needs_you)).count || "None open", detail: () => [viewPart("tasks", createElement("div", { className: "pj-cols" }, createElement(ProjectTasks, { view, act: agentActs })))] }] : []),
    // Running agents have their own rows above; this keeps past sessions and the custom agents.
    { key: "agents", title: "Sessions", sub: past.length ? `${past.length} past` : "None yet", detail: () => sessionTabs(p, running, rows) },
    { key: "branches", title: "Git", sub: "History and worktrees", detail: () => [] },
  ] });
  const num = (n: number) => createElement("span", { className: "num" }, `#${n}`);
  const prs = thr.prs(pid), issues = thr.issues(pid);
  const prLines = thr.prLines(agent, acts);
  const prItems: SplitItem[] = prLines.map(l => {
    const chip = prChip(l), pr = l.pr;
    return {
      key: `pr:${pr.number}`, opened: pr.created_at, lead: num(pr.number), title: pr.title, sub: prMeta(l, ms => formatAgo(ms, "short")).join(" · "), tags: pr.labels,
      right: chip && createElement(Chip, { kind: chip.cls, text: chip.text, spin: chip.cls === "running" }),
      menu: e => thr.prMenu(e, agent, acts, l),
      detail: () => [viewPart("thread", details.pane(pid, { ...pr, author: pr.mine ? "you" : pr.author, opened_at: pr.created_at }, thr.prButtons(agent, acts, l)))],
    };
  });
  const prKey = `${p.name}:prs`;
  const prFilter = labelFilter(prKey, prItems, i => i.tags, redrawProjects);
  groups.push({ title: "Pull requests", note: hostNote(prs, thr.prsError(pid), "No open pull requests."), controls: createElement(OpenListControls, { list: prKey, redraw: redrawProjects, issues: false }), labels: prFilter.labels, items: arranged(prKey, prFilter.items, i => i.opened) });
  const issueItems: SplitItem[] = thr.issueLines(agent, acts).map(l => ({
    key: `issue:${l.number}`, opened: l.issue?.opened_at, start: l.issue ? { key: `issue:${l.number}`, project: p.name, number: l.number, title: l.title, url: l.url } : undefined,
    lead: num(l.number), title: l.title,
    sub: l.issue ? issueMeta(l.issue, ms => formatAgo(ms, "short")) : `task: ${l.task!.title}`, tags: l.issue?.labels,
    right: l.task && createElement(Chip, { kind: l.task.state, text: l.task.state, spin: l.task.state === "running" }),
    menu: l.issue ? (e => thr.issueMenu(e, agent, acts, l.issue!)) : undefined,
    detail: () => [viewPart("thread", details.pane(pid, { number: l.number, title: l.title, url: l.url, author: l.issue?.author || "", opened_at: l.issue?.opened_at || 0 },
      l.issue ? thr.issueButtons(agent, acts, l.issue) : { buttons: [{ key: "tasks", label: "Tasks", primary: false, disabled: false, title: "", run: () => openTile("tasks") }], error: null }))],
  }));
  const issueKey = `${p.name}:issues`;
  const issueFilter = labelFilter(issueKey, issueItems, i => i.tags, redrawProjects);
  const issueShown = arranged(issueKey, issueFilter.items, i => i.opened);
  const issueNote = hostNote(issues, thr.issuesError(pid), "No open issues.");
  // Too many to list at the left: the row opens them at the right.
  project.push({ key: "issues", title: "Issues", sub: issueNote || `${issueItems.length} open`, detail: () => [] });
  // The front page: the board alone, or the Overview beside the list.
  const board = frontLayout("project") === "board";
  const overview = board ? null : projectOverview(p, running, { run: runItems, prs: prItems, issues: issueItems, prNote: hostNote(prs, thr.prsError(pid), "No open pull requests."), issueNote }, machineTile(rows, sync));
  top.unshift({ key: "overview", title: board ? "Board" : "Overview", sub: "", detail: () => overview ? [hostPart(overview)] : [] });
  groups.unshift({ title: "", items: top });
  // A review opens from Branches and an issue from Issues, each keeping its row lit.
  const all = groups.flatMap(g => g.items);
  const issueDetail = issueItems.find(i => i.key === pa.tile)?.detail;
  const picked = pa.tile === "review"
    ? { key: "branches", detail: () => [] }
    : issueDetail ? { key: "issues", detail: () => [viewPart("all-issues", createElement("button", { type: "button", className: "dbtn quiet small pj-back", onClick: () => openTile("issues") }, "All issues")), ...issueDetail()] }
    : all.find(i => i.key === pa.tile && i.detail) || all[0];
  const currentShell = projectShell(p.name);
  currentShell.navView = createElement(ProjectNavigation, { groups, picked: picked.key, openTile });

  const error = state.pview.get(p.name)?.error;
  const history = pa.tile === "branches" ? gitHistory.panels(p.name) : null;
  const showIssues = pa.tile === "issues";
  const showReview = pa.tile === "review";
  const issuesView = showIssues ? createElement("div", { className: "pj-det" }, createElement(ProjectIssues, {
    error: error ?? null, items: issueShown, filter: issueFilter.labels,
    controls: createElement(OpenListControls, { list: issueKey, redraw: redrawProjects, issues: true, flexItem: true }),
    start: createElement(OpenListStart, { list: issueKey, shown: issueShown.flatMap(item => item.start ?? []), redraw: redrawProjects, flexItem: true }),
    selection: (id: string) => issueSelection(issueKey, id),
    pick: (id: string, checked: boolean) => setIssueSelection(issueKey, id, checked, redrawProjects),
    openTile,
  })) : null;
  const det = history ? hostPart(history.detail) : showReview ? viewPart("review", revs.reviewDetail(p.name, error ?? null)) : issuesView ? viewPart("issues", issuesView) : viewPart("detail", createElement(Parts, { className: "pj-det", parts: [
    ...(error ? [viewPart("error", createElement(ErrorNote, { text: `Could not refresh the project: ${error}` }))] : []), ...(picked.detail?.() ?? []),
  ] }));
  const waiting = threads?.filter(t => t.needs_reply).length ?? 0;
  const cells = [
    { label: projectBranch(state.gitsync.get(p.name) ?? pages.get<GitRow[]>("gitsync", p.name), state.mac?.id || null) || "", text: "branch", key: "branches" },
    { label: "Actions", text: runs.tile(pid).count || "", key: "actions", bad: runs.tile(pid).rows[0]?.chip?.text === "failed" },
    { label: plural(running.length, "agent", "agents"), text: "running", key: "running" },
    { label: "Machines", text: machineTile(rows, sync).count || "", key: "machines" },
    { label: plural(prItems.length, "PR", "PRs"), text: waiting ? `${waiting} ${waiting === 1 ? "needs" : "need"} a reply` : "open", key: prFilter.items[0]?.key ?? prItems[0]?.key ?? "overview" },
    { label: plural(issueItems.length, "issue", "issues"), text: "open", key: "issues" },
    { label: "Sessions", text: past.length ? `${past.length} past` : "", key: "agents" },
  ];
  renderProjectShell(currentShell);
  const strip = createElement("div", { className: "cc-mstrip pj-strip" }, createElement(ProjectStatus, { cells, openTile }));
  // A picked row a filter hides keeps the list on screen, so the filter can be cleared.
  if (board && (!pa.tile || pa.tile === "overview")) {
    const pulls = prLines.flatMap(l => {
      const lane: Lane | null = l.needs_reply ? "needs" : l.review === "ready" ? "review" : null;
      return lane ? [{ key: `pr:${l.pr.number}`, lane, number: l.pr.number, title: l.pr.title, sub: prMeta(l, ms => formatAgo(ms, "short")).join(" · ") }] : [];
    });
    return [viewPart("strip", strip), viewPart("board", createElement(Parts, { className: "pj-board", parts: [
      ...(error ? [viewPart("error", createElement(ErrorNote, { text: `Could not refresh the project: ${error}` }))] : []), ...(cards ? [cards] : []),
      viewPart("lanes", createElement(ProjectBoard, { project: p.name, running, pulls, past, openTile })),
    ] }))];
  }

  return [viewPart("strip", strip), viewPart("split", createElement(Parts, { className: `${history ? "pj-split gh-layout" : "pj-split"}${pa.tile ? " picked" : ""}`,
    parts: [hostPart(history ? history.navigation : currentShell.navigation), det] }))];
}

/// The Sessions row's tab on screen.
let sessionTab: "sessions" | "agents" = "sessions";

/// Past sessions and custom agents in two tabs, so each gets the whole height.
function sessionTabs(p: Project, running: RunningAgent[], rows: WhereRow[]): PagePart[] {
  const list = sessionsList(p, running);
  const tabs = viewPart("session-tabs", createElement("div", { className: "cc-tabs pj-tabs", role: "tablist" }, createElement(SessionTabs, {
    selected: sessionTab, sessions: list.count, agents: customAgentsCount(rows), select: tab => { sessionTab = tab; redrawProjects(); } })));
  if (sessionTab === "agents") return [tabs, viewPart("custom-agents", customAgentsSection(rows))];

  const note = list.freshness && (list.freshness.spin || list.freshness.text) ? [viewPart("session-note", createElement(FreshNote, { freshness: list.freshness, className: " ss-updating" }))] : [];
  return [tabs, list.rows ? hostPart(sessionsHost(p.name, list.rows)) : viewPart("session-empty", createElement(EmptyPanel, { text: list.empty ?? "" })), ...note];
}

/// The agent view with terminal `t`, one of project `name`, in a pane tab with the keys.
function openTerminal(name: string, t: TerminalRow) {
  go("pane", { draft: { project: name, machine: t.machine_id, terminal: t } });
}

/// The project's Overview: what runs and ran on the left, the open work on
/// the right, each title opening that row of the list. Issues scroll in their box.
function projectOverview(p: Project, running: RunningAgent[], open: { run: SplitItem[]; prs: SplitItem[]; issues: SplitItem[]; prNote: string | undefined; issueNote: string | undefined }, where: ReturnType<typeof machineTile>) {
  const current = projectShell(p.name);
  current.overviewView = createElement(ProjectOverview, { ...open, machines: where, terminals: terms.list(p.name) || [],
    session: sessionsList(p, running), openTile, openTerminal: terminal => openTerminal(p.name, terminal) });
  return current.overview;
}
