import { useState } from "react";
import { Dropdown } from "../ui/dropdown.tsx";
import { PROJECT_SORTS, projectSortTime, type ProjectSort } from "../project/overview-state.ts";
import { lastProjectVisit, readProjectSort, saveProjectSort } from "../project/project-preferences.ts";
import { visibleRunning } from "../agents/closing.ts";
import { moveLabel } from "../agents/move-store.ts";
import { runningLineAt } from "../agents/place.ts";
import { byRecent, projectOf, runningKey, runningLine, type RunningAgent } from "../agents/running-state.ts";
import { sessionVisible } from "../agents/closing.ts";
import { Board, CardRow, type BoardCard } from "./lanes-view.tsx";
import { byProject, endedToday, laneOf } from "./lanes-state.ts";
import { frontLayout } from "../ui/skin.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { conflictList, conflictSub } from "../project/filesync.ts";
import { fsync } from "../project/sections.ts";
import { syncProjects } from "../project/syncpage.ts";
import { HarnessIcon, Icon, PROJECT_COLORS } from "../ui/icons.tsx";
import { type HomeView, needsLine } from "./home-state.ts";
import { homeSessions, openAsking } from "./home.tsx";
import { itemChips, newestFirst, projectsWithOpen, reviewStart, waitsOnYou } from "./openitems-state.ts";
import { openUrl } from "./openitems.ts";
import { projectRows } from "./sidebar.tsx";
import { state } from "./state.ts";
import { go } from "./views.tsx";
import { formatAgo } from "../ui/format.ts";
import { machinesReach } from "../machines/machine-state.ts";
import { isMobile } from "../ui/platform.ts";
import { Button, Section, EmptyPanel } from "../ui/kit.tsx";
import { PaintedEmpty } from "../ui/painted-art.tsx";
import { RunningStatus } from "../project/split-view.tsx";

/// What a running row says at its end: a move or a close, that it asks, or since when.
function runningStatus(row: RunningAgent, home: HomeView) {
  const moving = moveLabel(row);
  // Shown as last seen until its machine answers.
  const waiting = !!home.machines?.some(m => m.id === row.machine_id && m.connecting);
  return { foot: waiting ? <span className="cc-wait">connecting</span> : <RunningStatus row={row} />, waiting, why: moving?.why ?? null };
}

/// The project a session works in: the imported one that holds its folder.
const sessionProject = (s: { machine_id: string; cwd: string }, home: HomeView) => projectOf(s, home.projects || [])?.name ?? null;

/// What waits on the user: agents' questions, pull requests, sync conflicts.
function needsCards(home: HomeView): BoardCard[] {
  const cards: BoardCard[] = [];

  for (const need of home.needs_you) {
    const details = needsLine(need);
    const open = () => openAsking(need);
    const asks = home.running?.find(row => row.machine_id === need.machine_id && row.id === need.session);
    cards.push({
      project: asks ? sessionProject(asks, home) : null,
      key: runningKey({ machine_id: need.machine_id, id: need.session }), lane: "needs", session: true,
      lead: <HarnessIcon agent={need.agent} />, title: details.title, sub: details.sub, foot: null,
      action: { kind: "run", label: details.button, primary: true, run: open }, dim: false, hint: null, open, menu: null,
    });
  }

  const repos = new Map(projectsWithOpen(state.git, projectRows() || []).map(repo => [repo.key, repo]));

  for (const item of (state.git?.items || []).filter(waitsOnYou).sort(newestFirst)) {
    const open = () => openUrl(item.url);
    const project = repos.get(`${item.host} ${item.repo}`)?.project ?? null;
    // A review can be read on the host, or in a project handed to an agent.
    const review = project && (() => { go("pane", { agentnew: reviewStart(item, project, home.machines?.filter(m => m.can_host !== false)) }); });
    cards.push({ key: item.url, lane: "needs", project: project?.name ?? item.repo, session: false, lead: <span style={{ display: "contents" }}><Icon name={item.kind === "pull_request" ? "pr" : "issue"} /></span>,
      title: `#${item.number} ${item.title}`, sub: [item.repo, itemChips(item).map(c => c.text).join(", ")].filter(Boolean).join(" · "), foot: null,
      action: item.review_requested && review ? { kind: "menu", label: "Review", items: [{ label: "Open", on: open }, { label: "Review with agent", on: review }] } : { kind: "run", label: "Open", primary: false, run: open }, dim: false, hint: item.url, open, menu: null });
  }

  // A project's conflicts share one card: its Machines section lists them.
  for (const project of syncProjects()) {
    const detail = fsync.detail(project), conflicts = conflictList(detail), first = conflicts[0];
    if (!first) continue;
    const one = conflicts.length === 1, open = () => go("projects", { project, tile: "machines" });
    cards.push({ key: `${project}:conflicts`, lane: "needs", project, session: false, lead: <span style={{ display: "contents" }}><Icon name="sync" /></span>,
      title: one ? first.path : `${conflicts.length} files have two copies`, sub: one ? conflictSub(project, first, detail?.machines ?? []) : project, foot: null,
      action: { kind: "run", label: "Resolve", primary: false, run: open }, dim: false, hint: conflicts.map(conflict => conflict.path).join("\n"), open, menu: null });
  }

  return cards;
}

/// Everything that runs, the latest first.
function runningCards(home: HomeView): BoardCard[] {
  const projects = home.projects || [];
  return byRecent(visibleRunning(home.running)).map(row => {
    const { title, sub } = runningLineAt(row, projects);
    const status = runningStatus(row, home);
    return { key: runningKey(row), lane: laneOf(row.status), project: sessionProject(row, home), session: true, lead: <HarnessIcon agent={row.agent} />,
      title, sub, foot: status.foot, action: null, dim: status.waiting, hint: status.why,
      open: () => go("pane", { agent: row }),
      menu: event => { void sessionMenu(event, row, s => go("pane", { agent: s })); } };
  });
}

/// Home's board: what waits on the user, then every session once, by its lane.
function boardCards(home: HomeView, needs: BoardCard[], running: BoardCard[]): BoardCard[] {
  const projects = home.projects || [];
  const shown = new Set(needs.map(card => card.key));
  const cards = [...needs];

  for (const card of running) {
    if (shown.has(card.key)) continue;
    shown.add(card.key);
    cards.push(card);
  }

  for (const session of endedToday(homeSessions().filter(sessionVisible), Date.now()).slice(0, DONE_SHOWN)) {
    const key = runningKey(session);
    if (shown.has(key)) continue;
    const { title, sub } = runningLine(session, projects);
    cards.push({ key, lane: "done", project: sessionProject(session, home), session: true, lead: <HarnessIcon agent={session.agent} />, title, sub, foot: formatAgo(session.last_activity, "since"),
      action: null, dim: false, hint: null, open: () => go("pane", { agent: { kind: "agent", ...session } }), menu: null });
  }

  return cards;
}

/// The Done today lane keeps to its latest sessions; the Sessions list has the rest.
const DONE_SHOWN = 8;

export function HomeOverview({ home }: { home: HomeView }) {
  const [sort, setSort] = useState(() => readProjectSort("home"));
  const changeSort = (value: ProjectSort) => {
    saveProjectSort("home", value);
    setSort(value);
  };

  const list = frontLayout("home") === "list";
  const needs = needsCards(home), running = runningCards(home);
  const cards = list ? [...needs, ...running] : boardCards(home, needs, running);
  const empty = !cards.length;
  // A row's dot matches the project's in the sidebar.
  const listed = projectRows() || [];
  const activity = new Map<string, number>();

  for (const session of [...(home.running ?? []), ...homeSessions()]) {
    const name = sessionProject(session, home);
    if (name !== null) activity.set(name, Math.max(activity.get(name) ?? 0, session.last_activity ?? session.started_at ?? 0));
  }

  for (const repo of projectsWithOpen(state.git, listed)) {
    const name = repo.project?.name ?? repo.repo;
    activity.set(name, Math.max(activity.get(name) ?? 0, repo.last));
  }

  const names = new Set(cards.flatMap(card => card.project === null ? [] : [card.project]));
  const recency = sort === "default" ? null : new Map([...names].map(name => [name, projectSortTime(sort, { last: activity.get(name) ?? null }, null, lastProjectVisit(name))]));
  const ordered = (rows: BoardCard[]) => recency === null ? rows : byProject(rows, recency).flatMap(row => row.cards);
  const rows = (project: string) => {
    const at = listed.findIndex(row => row.name === project);
    return { color: at < 0 ? null : PROJECT_COLORS[at % PROJECT_COLORS.length], open: listed[at]?.imported ? () => { go("projects", { project }); } : null };
  };

  return <>
    {!empty && <div className="home-sort"><Dropdown label="Sort projects" options={PROJECT_SORTS} value={sort} onChange={changeSort} icon={<Icon name="sort"/>}/></div>}
    {list ? !!needs.length && <Section id="sec-needs" title="Needs you" count={needs.length}>{ordered(needs).map(card => <CardRow key={card.key} card={card}/>)}</Section>
      : !!cards.length && <div id="sec-needs"><Board key={sort} cards={ordered(cards)} more={{ working: () => go("pane", "agents"), done: () => go("pane", "agents") }} rows={isMobile() ? null : rows} recency={recency}/></div>}
    {list && !!running.length && <Section id="sec-running" title={<Button className="lk" onClick={() => go("pane", "agents")}>Running</Button>} count={running.length}>{ordered(running).map(card => <CardRow key={card.key} card={card}/>)}</Section>}
    {empty && <Empty home={home} />}
  </>;
}

/// Nothing to show: nothing runs, or no machine has answered yet.
function Empty({ home }: { home: HomeView }) {
  const reach = machinesReach(home.machines ?? []);
  if (reach !== "reached") return <EmptyPanel text={reach === "connecting" ? "Connecting to your machines…" : "No machine is online."} />;
  return <PaintedEmpty><b>{state.home ? "Nothing running" : "Start an agent"}</b><span>{state.home ? "Start an agent on this computer, or add a machine to run it there." : "Your machines connect in the background."}</span><Button look="primary" small onClick={() => go("projects", { project: null })}>Start an agent</Button></PaintedEmpty>;
}
