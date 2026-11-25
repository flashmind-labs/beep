import { moveStore } from "../agents/move-store.ts";
import { MACHINES } from "../machines/machine-store.ts";
import { type ReactNode } from "react";
import { createPortal } from "react-dom";
import { removeWindowPart, showWindowParts } from "../center/window-root.tsx";
import { ProjectNavigation, ProjectOverview, ProjectStatus, RunningStatus, SessionTabs } from "./split-view.tsx";
import { projectCards } from "./lanes.tsx";
import { isMobile } from "../ui/platform.ts";
import { Board } from "../center/lanes-view.tsx";
import { frontLayout } from "../ui/skin.ts";
import { ProjectIssues } from "./issues-view.tsx";
import { OpenListControls, OpenListStart } from "./openlist-view.tsx";
// A project page's split: the list of PRs, issues and machines on the
// left, the picked row's view on the right.
import { sessionVisible } from "../agents/closing.ts";
import { formatAgo, plural } from "../ui/format.ts";
import { projectPastSessions, type Project, type WhereRow } from "./projects-state.ts";
import { projectRef } from "./localid.ts";
import { FreshNote, Parts, hostPart, retainedHost, viewPart, type PagePart } from "./page-view.tsx";
import type { TerminalRow } from "../terminals/terminals.tsx";
import { terminalTile } from "../terminals/terminal-list.ts";
import { fileSyncTile } from "./filesync.ts";
import { machineTile } from "./board.ts";
import { issueMeta, prMeta, sortedPrs } from "./threads.ts";
import { arranged, issueSelection, setIssueSelection } from "./openlist.ts";
import type { StartEntry } from "./openlist-state.ts";
import { gitOf, sessionsOf, state } from "../center/state.ts";
import { projectBranch } from "./gitsync-state.ts";
import { boxOf, go } from "../center/views.tsx";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { agentsSection, customAgentsCount, customAgentsSection, whereSection } from "./projects.tsx";
import { openUrl } from "../center/openitems.ts";
import { pa, terms, fsync, gitHistory, thr, details, runs, redrawProjects, repoActs } from "./sections.ts";
import { sessionsHost, sessionsList, sessionsDue, loadSessions } from "./loads.tsx";
import { byRecent, type RunningAgent, runningKey } from "../agents/running-state.ts";
import { runningLineAt } from "../agents/place.ts";
import { HarnessIcon } from "../ui/icons.tsx";
import { Button, Chip, EmptyPanel } from "../ui/kit.tsx";
import { WandererArt } from "../ui/painted-art.tsx";
import { boardFiltered, IssueOwners, IssueOwnersCell, myIssues, TeamFilter, TeamPage, teamIssues, teamNote, teamOf } from "./team-view.tsx";

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
  showWindowParts([{ part: "project-shell", host: null, identity: current.project, content: <>
    {createPortal(current.navView, current.navigation, "navigation")}{createPortal(current.overviewView, current.overview, "overview")}</> }]);
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

/// The split shows for every imported project.
export function splitOn(p: Project) {
  return p.imported;
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
  if (pa.tile !== "machines") terms.stop();
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
  sub: ReactNode;
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
  return list?.length ? undefined : none;
}

/// The project page: everything going on in one list at the left, the
/// picked row's detail and actions at the right.
export function projectSplit(p: Project, rows: WhereRow[], known: boolean, running: RunningAgent[]): PagePart[] {
  const agent = projectRef(p.name);
  const pid = agent.project_id;
  const asked = pa.run;

  if (asked?.project === p.name) {
    pa.run = null;
    queueMicrotask(() => runs.show(pid, asked.id, asked.host));
  }

  const sessions = sessionsOf(p.name);
  if (sessions === undefined || sessionsDue.has(p.name)) loadSessions(p.name);
  if (terms.list(p.name) === undefined) terms.load(p.name);
  thr.ensure(pid);
  const acts = repoActs(p);
  const sync = state.sync.filter(s => s.project === p.name);
  const syncCount = fileSyncTile(fsync.forTile(p.name), sync, new Map(rows.map(m => [m.id, m.name]))).count;
  const groups: SplitGroup[] = [];
  // Overview and Running head the list, each one line.
  const top: SplitItem[] = [];
  const project: SplitItem[] = [
    { key: "actions", title: "Actions", sub: runs.tile(pid).count || "", detail: () => [viewPart("actions", runs.section(agent))] },
    // A machine's terminals sit below it, as on Home.
    { key: "machines", title: "Machines", sub: [syncCount || machineTile(rows, sync).count, terminalTile(terms.list(p.name), ms => formatAgo(ms, "short")).count].filter(Boolean).join(" · "), detail: () => [viewPart("where", whereSection(p, rows, known))] },
  ];
  const runItems: SplitItem[] = byRecent(running).map(r => ({
    key: `run:${runningKey(r)}`, lead: <HarnessIcon agent={r.agent} />, ...runningLineAt(r, state.home?.projects || []),
    right: <RunningStatus row={r} />, open: () => go("pane", { agent: r }),
    menu: e => void sessionMenu(e, r, s => go("pane", { agent: s })),
  }));
  if (runItems.length) top.push({ key: "running", title: "Running", sub: String(runItems.length), detail: () => [viewPart("running", agentsSection(running))] });
  // A phone neither invites nor joins: its board shows the team's cards alone.
  const team = isMobile() ? null : teamNote(p.name);
  if (team !== null) project.unshift({ key: "team", title: "Team", sub: team, detail: () => [viewPart("team", <TeamPage project={p.name} machines={p.machines.map(m => m.name)} running={running.length}/>)] });
  groups.push({ title: "", items: project });
  const past = sessions ? projectPastSessions(sessions, running, [...moveStore.getState().records.values()]).filter(sessionVisible) : [];
  groups.push({ title: "", items: [
    // Running agents have their own rows above; this keeps past sessions and the custom agents.
    { key: "agents", title: "Sessions", sub: past.length ? `${past.length} past` : "None yet", detail: () => sessionTabs(p, running, rows) },
    { key: "branches", title: "Git", sub: "History and worktrees", detail: () => [] },
  ] });
  const num = (n: number) => <span className="num">{`#${n}`}</span>;
  const prs = thr.prs(pid), issues = thr.issues(pid);
  // A link to one of the project's open pull requests opens it here.
  const openLink = (url: string) => {
    const pr = prs?.find(open => open.url === url);
    if (pr) openTile(`pr:${pr.number}`); else void openUrl(url);
  };
  const prItems: SplitItem[] = sortedPrs(prs || []).map(pr => ({
    key: `pr:${pr.number}`, opened: pr.created_at, lead: num(pr.number), title: pr.title, sub: prMeta(pr, ms => formatAgo(ms, "short")).join(" · "), tags: pr.labels,
    right: pr.draft ? <Chip kind="" text="draft" /> : undefined,
    menu: e => thr.prMenu(e, agent, acts, pr),
    detail: () => [viewPart("thread", details.pane(pid, { ...pr, author: pr.mine ? "you" : pr.author, opened_at: pr.created_at }, thr.prButtons(agent, acts, pr), openLink))],
  }));
  const prKey = `${p.name}:prs`;
  const prFilter = labelFilter(prKey, prItems, i => i.tags, redrawProjects);
  groups.push({ title: "Pull requests", note: hostNote(prs, thr.prsError(pid), "No open pull requests."), controls: <OpenListControls list={prKey} redraw={redrawProjects} issues={false} />, labels: prFilter.labels, items: arranged(prKey, prFilter.items, i => i.opened) });
  const issueItems: SplitItem[] = teamIssues(p.name, issues || []).map(issue => ({
    key: `issue:${issue.number}`, opened: issue.opened_at, start: { key: `issue:${issue.number}`, project: p.name, number: issue.number, title: issue.title, url: issue.url },
    title: issue.title, sub: <>{num(issue.number)} · {issueMeta(issue, ms => formatAgo(ms, "short"))}</>, tags: issue.labels,
    right: <><IssueOwnersCell project={p.name} issue={issue}/>{issue.linked_pr && <Chip kind="wait" text="PR" title="A pull request is linked to it" />}</>,
    menu: e => thr.issueMenu(e, agent, acts, issue),
    detail: () => [viewPart("thread", details.pane(pid, { number: issue.number, title: issue.title, url: issue.url, author: issue.author, opened_at: issue.opened_at }, thr.issueButtons(agent, acts, issue), openLink))],
  }));
  const issueKey = `${p.name}:issues`;
  const issueFilter = labelFilter(issueKey, issueItems, i => i.tags, redrawProjects);
  const issueShown = arranged(issueKey, issueFilter.items, i => i.opened);
  const issueNote = hostNote(issues, thr.issuesError(pid), "No open issues.");
  // Too many to list at the left: the row opens them at the right.
  project.push({ key: "issues", title: "Issues", sub: issueNote || `${issues?.length ?? 0} open`, detail: () => [] });
  // The front page: the board alone, or the Overview beside the list.
  const board = frontLayout("project") === "board";
  const overview = board ? null : projectOverview(p, running, { run: runItems, prs: prItems, issues: issueItems, prNote: hostNote(prs, thr.prsError(pid), "No open pull requests."), issueNote }, machineTile(rows, sync));
  top.unshift({ key: "overview", title: board ? "Board" : "Overview", sub: "", detail: () => overview ? [hostPart(overview)] : [] });
  groups.unshift({ title: "", items: top });
  // An issue opens from Issues, keeping its row lit.
  const all = groups.flatMap(g => g.items);
  const issueDetail = issueItems.find(i => i.key === pa.tile)?.detail;
  // Pull requests open the first one, or say there is none.
  const firstPr = prFilter.items[0] ?? prItems[0];
  const prsPicked = pa.tile !== "prs" ? null : firstPr?.detail ? { key: firstPr.key, detail: firstPr.detail }
    : { key: "prs", detail: () => [viewPart("no-prs", <EmptyPanel text={hostNote(prs, thr.prsError(pid), "No open pull requests.") ?? ""} />)] };
  const picked = prsPicked ?? (issueDetail ? { key: "issues", detail: () => [viewPart("all-issues", <Button look="quiet" small className="pj-back" onClick={() => openTile("issues")}>All issues</Button>), ...issueDetail()] }
    : all.find(i => i.key === pa.tile && i.detail) || all[0]);
  const currentShell = projectShell(p.name);
  currentShell.navView = <ProjectNavigation groups={groups} picked={picked.key} openTile={openTile} />;

  const history = pa.tile === "branches" ? gitHistory.panels(p.name) : null;
  const showIssues = pa.tile === "issues";
  const issuesView = showIssues ? <div className="pj-det"><ProjectIssues
    items={issueShown} filter={issueFilter.labels} owners={<IssueOwners project={p.name} issues={issues || []}/>}
    controls={<OpenListControls list={issueKey} redraw={redrawProjects} issues flexItem />}
    start={<OpenListStart list={issueKey} shown={issueShown.flatMap(item => item.start ?? [])} redraw={redrawProjects} flexItem />}
    selection={(id: string) => issueSelection(issueKey, id)}
    pick={(id: string, checked: boolean) => setIssueSelection(issueKey, id, checked, redrawProjects)}
    openTile={openTile}
  /></div> : null;
  const det = history ? hostPart(history.detail) : issuesView ? viewPart("issues", issuesView) : viewPart("detail", <Parts className="pj-det" parts={picked.detail?.() ?? []} />);
  const mine = myIssues(p.name, issues || []);
  const cells = [
    { label: projectBranch(gitOf(p.name), MACHINES.mac?.id || null) || "", text: "branch", key: "branches" },
    { label: "Actions", text: runs.tile(pid).count || null, key: "actions", bad: runs.tile(pid).rows[0]?.chip?.text === "failed" },
    { label: plural(running.length, "agent", "agents"), text: "running", key: "running" },
    { label: "Team", text: team, key: "team" },
    { label: "Machines", text: machineTile(rows, sync).count || null, key: "machines" },
    { label: plural(prItems.length, "PR", "PRs"), text: "open", key: "prs" },
    { label: plural(issues?.length ?? 0, "issue", "issues"), text: mine ? `open, ${mine} yours` : "open", key: "issues" },
    { label: "Sessions", text: past.length ? `${past.length} past` : null, key: "agents" },
  ];
  renderProjectShell(currentShell);
  const strip = <div className="cc-mstrip pj-strip"><ProjectStatus cells={cells} openTile={openTile} /></div>;
  // A picked row a filter hides keeps the list on screen, so the filter can be cleared.
  if (board && (!pa.tile || pa.tile === "overview")) {
    const cards = projectCards({ project: p.name, url: p.url, running, past, openTile });
    const view = teamOf(p.name);
    // A phone has no bar to give the page to; a filter that hides every card keeps its chips.
    pa.blank = !cards.length && !boardFiltered(p.name) && !isMobile();
    if (pa.blank) return [viewPart("strip", strip)];
    return [viewPart("strip", strip), viewPart("board", <Parts className="pj-board" parts={[
      ...view?.kind === "team" ? [viewPart("team-filter", <TeamFilter project={p.name} view={view}/>)] : [],
      viewPart("lanes", <Board cards={cards} more={{ working: () => openTile("running"), done: () => openTile("agents") }} beside={<WandererArt/>}/>),
    ]} />)];
  }

  return [viewPart("strip", strip), viewPart("split", <Parts className={`${history ? "pj-split gh-layout" : "pj-split"}${pa.tile ? " picked" : ""}`}
    parts={[hostPart(history ? history.navigation : currentShell.navigation), det]} />)];
}

/// The Sessions row's tab on screen.
let sessionTab: "sessions" | "agents" = "sessions";

/// Past sessions and custom agents in two tabs, so each gets the whole height.
function sessionTabs(p: Project, running: RunningAgent[], rows: WhereRow[]): PagePart[] {
  const list = sessionsList(p, running);
  const tabs = viewPart("session-tabs", <div className="cc-tabs pj-tabs" role="tablist"><SessionTabs
    selected={sessionTab} sessions={list.count} agents={customAgentsCount(rows)} select={tab => { sessionTab = tab; redrawProjects(); }} /></div>);
  if (sessionTab === "agents") return [tabs, viewPart("custom-agents", customAgentsSection(rows))];

  const note = list.freshness && (list.freshness.spin || list.freshness.text) ? [viewPart("session-note", <FreshNote freshness={list.freshness} className=" ss-updating" />)] : [];
  return [tabs, list.rows ? hostPart(sessionsHost(p.name, list.rows)) : viewPart("session-empty", <EmptyPanel text={list.empty ?? ""} />), ...note];
}

/// The agent view with terminal `t`, one of project `name`, in a pane tab with the keys.
function openTerminal(name: string, t: TerminalRow) {
  go("pane", { draft: { project: name, machine: t.machine_id, terminal: t } });
}

/// The project's Overview: what runs and ran on the left, the open work on
/// the right, each title opening that row of the list. Issues scroll in their box.
function projectOverview(p: Project, running: RunningAgent[], open: { run: SplitItem[]; prs: SplitItem[]; issues: SplitItem[]; prNote: string | undefined; issueNote: string | undefined }, where: ReturnType<typeof machineTile>) {
  const current = projectShell(p.name);
  current.overviewView = <ProjectOverview {...open} machines={where} terminals={terms.list(p.name) || []}
    session={sessionsList(p, running)} openTile={openTile} openTerminal={terminal => openTerminal(p.name, terminal)} />;
  return current.overview;
}
