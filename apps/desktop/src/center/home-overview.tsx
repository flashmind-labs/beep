import { useEffect, useState, type CSSProperties } from "react";
import { closeSession, sessionVisible, visibleRunning } from "../agents/closing.ts";
import { moveLabel } from "../agents/move-store.ts";
import { runningLineAt } from "../agents/place.ts";
import { byRecent, projectOf, runningKey, runningLine, type RunningAgent } from "../agents/running-state.ts";
import { Board, CardRow, type BoardCard } from "./lanes-view.tsx";
import { LANES, endedToday, laneOf } from "./lanes-state.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { conflictList, conflictSub } from "../project/filesync.ts";
import { fsync } from "../project/sections.ts";
import { syncProjects } from "../project/syncpage.ts";
import { HarnessIcon, Icon, PROJECT_COLORS } from "../ui/icons.tsx";
import { type HomeView, needsLine } from "./home-state.ts";
import { answerNeed, homeSessions, openAsking } from "./home.tsx";
import { itemChips, newestFirst, projectsWithOpen, waitsOnYou } from "./openitems-state.ts";
import { continueButton, openUrl, reviewButton } from "./openitems.ts";
import { loadFound, projectRows, projectWindowProps } from "./sidebar.tsx";
import { HomeAssistant } from "./home-assistant.tsx";
import { state } from "./state.ts";
import { go } from "./views.tsx";
import { formatAgo } from "../ui/format.ts";
import { machinesReach } from "../machines/machine-state.ts";
import { Button, Section, EmptyPanel } from "../ui/kit.tsx";
import { Segmented } from "../ui/aria-kit.tsx";
import { PaintedEmpty } from "../ui/painted-art.tsx";
import { RunningStatus, workLine } from "../project/split-view.tsx";
import { sessions } from "../agents/sessions.ts";
import { isAssistant } from "../agents/assistant-state.ts";

/// What a running row says at its end: a move or a close, that it asks, or since when. `doing`
/// puts its step there while it works.
function runningStatus(row: RunningAgent, home: HomeView, doing: boolean) {
  const moving = moveLabel(row);
  // Shown as last seen until its machine answers.
  const waiting = !!home.machines?.some(m => m.id === row.machine_id && m.connecting);
  return { foot: waiting ? <span className="cc-wait">connecting</span> : <RunningStatus row={row} doing={doing}/>, waiting, why: moving?.why ?? null };
}

/// A project's colour, the same as its dot in the sidebar; null for no listed project.
function projectColor(project: string | null) {
  const at = project === null ? -1 : (projectRows() || []).findIndex(row => row.name === project);
  return at < 0 ? null : PROJECT_COLORS[at % PROJECT_COLORS.length];
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
    const project = asks ? sessionProject(asks, home) : null;
    cards.push({
      project, color: projectColor(project),
      key: runningKey({ machine_id: need.machine_id, id: need.session }), lane: "needs", session: true,
      lead: <HarnessIcon agent={need.agent} />, title: details.title, sub: [project, details.sub].filter(Boolean).join(" · "), foot: null,
      // A question answered in words, with nothing to allow, is answered in its chat.
      action: need.question.options.some(option => option.allow) ? { kind: "answer", question: need.question, answer: reply => answerNeed(need, reply) } : { kind: "run", label: "Answer", primary: true, run: open },
      dim: false, hint: null, open, menu: null,
    });
  }

  const repos = new Map(projectsWithOpen(state.git, projectRows() || []).map(repo => [repo.key, repo]));

  for (const item of (state.git?.items || []).filter(waitsOnYou).sort(newestFirst)) {
    const open = () => openUrl(item.url);
    const project = repos.get(`${item.host} ${item.repo}`)?.project ?? null;
    // A review can be read on the host, or in a project handed to an agent,
    // and the user's own PR goes on in a project's agent.
    const machines = home.machines?.filter(m => m.can_host !== false);
    const agent = !project || item.kind !== "pull_request" ? null
      : item.review_requested ? reviewButton(item, project, machines)
      : item.authored ? continueButton(item, project, machines) : null;

    cards.push({ key: item.url, lane: "needs", project: project?.name ?? item.repo, color: projectColor(project?.name ?? null), session: false, lead: <span style={{ display: "contents" }}><Icon name={item.kind === "pull_request" ? "pr" : "issue"} /></span>,
      title: `#${item.number} ${item.title}`, sub: [item.repo, itemChips(item).map(c => c.text).join(", ")].filter(Boolean).join(" · "), foot: null,
      action: agent ? { kind: "agent", item: agent } : { kind: "run", label: "Open", primary: false, run: open }, dim: false, hint: item.url, open, menu: null });
  }

  // A project's conflicts share one card: its Machines section lists them.
  for (const project of syncProjects()) {
    const detail = fsync.detail(project), conflicts = conflictList(detail), first = conflicts[0];
    if (!first) continue;
    const one = conflicts.length === 1, open = () => go("projects", { project, tile: "machines" });
    cards.push({ windowProps: projectWindowProps(project), key: `${project}:conflicts`, lane: "needs", project, color: projectColor(project), session: false, lead: <span style={{ display: "contents" }}><Icon name="sync" /></span>,
      title: one ? first.path : `${conflicts.length} files have two copies`, sub: one ? conflictSub(project, first, detail?.machines ?? []) : project, foot: null,
      action: { kind: "run", label: "Resolve", primary: false, run: open, windowProps: projectWindowProps(project) }, dim: false, hint: conflicts.map(conflict => conflict.path).join("\n"), open, menu: null });
  }

  return cards;
}

/// Everything that runs but the assistant's chats, the latest first. A `grouped` card says where
/// it runs and what it does: its step, or the first line of its last reply.
function runningCards(home: HomeView, assistants: ReadonlySet<string>, grouped: boolean): BoardCard[] {
  const projects = home.projects || [];
  return byRecent(visibleRunning(home.running).filter(row => !assistants.has(runningKey(row)))).map(row => {
    const line = runningLineAt(row, projects), work = grouped ? workLine(row) : "";
    const title = line.title, sub = !grouped ? line.sub : work ? <>{row.machine_name} · {work}</> : row.machine_name;
    const status = runningStatus(row, home, !grouped), lane = laneOf(row.status), project = sessionProject(row, home);
    // A finished turn can be closed from its card, so Ready to review empties.
    const close = () => { closeSession(row).catch(error => console.warn("close:", error)); };
    return { key: runningKey(row), lane, project, color: projectColor(project), session: true, lead: <HarnessIcon agent={row.agent} />,
      title, sub, foot: status.foot, action: lane === "review" ? { kind: "close", run: close } as const : null, dim: status.waiting, hint: status.why,
      open: () => go("pane", { agent: row }),
      menu: event => { void sessionMenu(event, row, s => go("pane", { agent: s })); } };
  });
}

/// Home's board: what waits on the user, then every session once, by its lane.
function boardCards(home: HomeView, needs: BoardCard[], running: BoardCard[], assistants: ReadonlySet<string>): BoardCard[] {
  const projects = home.projects || [];
  const shown = new Set(needs.map(card => card.key));
  const cards = [...needs];

  for (const card of running) {
    if (shown.has(card.key)) continue;
    shown.add(card.key);
    cards.push(card);
  }

  for (const session of endedToday(homeSessions().filter(s => sessionVisible(s) && !assistants.has(runningKey(s))), Date.now()).slice(0, DONE_SHOWN)) {
    const key = runningKey(session);
    if (shown.has(key)) continue;
    const { title, sub } = runningLine(session, projects);
    const project = sessionProject(session, home);
    cards.push({ key, lane: "done", project, color: projectColor(project), session: true, lead: <HarnessIcon agent={session.agent} />, title, sub, foot: formatAgo(session.last_activity, "since"),
      action: null, dim: false, hint: null, open: () => go("pane", { agent: { kind: "agent", ...session } }), menu: null });
  }

  return cards;
}

/// The Done today lane keeps to its latest sessions; the Sessions list has the rest.
const DONE_SHOWN = 8;

/// Home's lanes below the queue: Needs you is the queue.
const QUEUED_LANES = LANES.filter(({ lane }) => lane !== "needs");

/// How Home lays its sessions out: by lane, or under their projects.
const GROUPINGS = [{ id: "lanes", label: "Lanes" }, { id: "projects", label: "Projects" }] as const;
type Grouping = typeof GROUPINGS[number]["id"];
const GROUPING_KEY = "beep.home.grouping";
const readGrouping = (): Grouping => { try { return GROUPINGS.find(grouping => grouping.id === localStorage.getItem(GROUPING_KEY))?.id ?? "lanes"; } catch { return "lanes"; } };

/// Home: what waits on the user as a queue, answered in place, over the lanes
/// of every other session, or over the open sessions as rows under their projects.
export function HomeOverview({ home }: { home: HomeView }) {
  const [grouping, setGrouping] = useState(readGrouping);
  const changeGrouping = (next: Grouping) => { setGrouping(next); try { localStorage.setItem(GROUPING_KEY, next); } catch { /* kept for this launch only */ } };
  const grouped = grouping === "projects";
  // The assistant's chats have their own page.
  const assistants = new Set(sessions.filter(isAssistant).map(runningKey));
  const cards = boardCards(home, needsCards(home), runningCards(home, assistants, grouped), assistants);
  // A project shows while a session is open in it: the ended ones are in the lanes.
  const queue = cards.filter(card => card.lane === "needs"), lanes = cards.filter(card => card.lane !== "needs" && !(grouped && card.lane === "done"));

  return <>
    {!!cards.length && <div className="home-sort"><Segmented label="Group sessions" options={GROUPINGS} value={grouping} onChange={changeGrouping}/></div>}
    {!!queue.length && <Section id="sec-needs" title="Needs you" count={queue.length}>{queue.map(card => <CardRow key={card.key} card={card}/>)}</Section>}
    {!!lanes.length && <div id="sec-running">{grouped ? <ByProject cards={lanes}/> : <Board cards={lanes} lanes={QUEUED_LANES} spread more={{ working: () => go("pane", "agents"), done: () => go("pane", "agents") }}/>}</div>}
    {!queue.length && !lanes.length && <Empty home={home} />}
  </>;
}

/// The open sessions as rows under their projects, in two columns: a project by its latest
/// session, with the sessions in no project last. A project lists its first `ROWS_SHOWN`.
function ByProject({ cards }: { cards: readonly BoardCard[] }) {
  const groups = new Map<string | null, BoardCard[]>();

  for (const card of cards) {
    const group = groups.get(card.project);
    if (group) group.push(card);
    else groups.set(card.project, [card]);
  }

  const boxes = [...groups].sort(([a], [b]) => Number(a === null) - Number(b === null));

  return <div className="bd-projects">{boxes.map(([project, rows]) => {
    // A listed project has its colour, and its name opens its page, where the rest of it is.
    const color = rows[0].color, listed = project !== null && color !== null;
    const all = listed ? () => go("projects", { project }) : () => go("pane", "agents");
    const title = !listed ? project ?? "Other folders"
      : <Button look="quiet" small {...projectWindowProps(project).clicks} {...projectWindowProps(project).menu} onClick={all}><span className="pj-swatch" style={{ "--c": color } as CSSProperties}/>{project}</Button>;

    return <Section key={project ?? ""} title={title} count={rows.length} action={rows.length > ROWS_SHOWN ? <Button look="quiet" small onClick={all}>All {rows.length}</Button> : null}>
      {rows.slice(0, ROWS_SHOWN).map(card => <CardRow key={card.key} card={{ ...card, color: null }}/>)}
    </Section>;
  })}</div>;
}

/// The sessions a project lists on Home.
const ROWS_SHOWN = 5;

/// Nothing to show: nothing runs, or no machine has answered yet. Before any
/// project, the assistant and the found projects.
function Empty({ home }: { home: HomeView }) {
  const first = home.projects?.length === 0;
  useEffect(() => { if (first && !state.found) void loadFound(); }, [first]);
  const reach = machinesReach(home.machines ?? []);
  if (reach !== "reached") return <EmptyPanel text={reach === "connecting" ? "Connecting to your machines…" : "No machine is online."} />;
  if (first) return <HomeAssistant found={state.found} />;
  return <PaintedEmpty><b>{state.home ? "Nothing running" : "Start an agent"}</b><span>{state.home ? "Start an agent on this computer, or add a machine to run it there." : "Your machines connect in the background."}</span><Button look="primary" small onClick={() => go("projects", { project: null })}>Start an agent</Button></PaintedEmpty>;
}
