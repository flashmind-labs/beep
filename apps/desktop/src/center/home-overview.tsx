import { S } from "../bar/core.ts";
import { visibleRunning } from "../agents/closing.ts";
import { closingNote } from "../agents/closing.ts";
import { moveLabel } from "../agents/move-store.ts";
import { runningLineAt } from "../agents/place.ts";
import { byRecent, runningKey, runningLine, runningWhen, type RunningRow } from "../agents/running-state.ts";
import { sessionVisible } from "../agents/closing.ts";
import { Board, CardRow, frontLayout, type BoardCard } from "./lanes-view.tsx";
import { endedToday, laneOf } from "./lanes-state.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { conflictList, conflictSub } from "../project/filesync.ts";
import { fsync } from "../project/sections.ts";
import { syncProjects } from "../project/syncpage.ts";
import { HarnessIcon, Icon } from "../ui/icons.tsx";
import { type HomeView, needsLine } from "./home-state.ts";
import { answerChat, cardKey, homeSessions, dismissUpdates, openAsking, openUpdate, scheduleText, whenOf } from "./home.ts";
import { itemChips, newestFirst, waitsOnYou } from "./openitems-state.ts";
import { openUrl } from "./openitems.ts";
import { state } from "./state.ts";
import { go, openResearch } from "./views.ts";
import { formatAgo } from "../ui/format.ts";
import { LineRow, Section } from "../ui/kit.tsx";

/// What a running row says at its end: a move or a close, that it asks, or since when.
function runningStatus(row: RunningRow, home: HomeView) {
  const moving = row.kind === "agent" ? moveLabel(row) : null;
  const closing = row.kind === "agent" ? closingNote(row) : null;
  const working = row.kind === "agent" && ["working", "running", "starting"].includes(row.status);
  // Shown as last seen until its machine answers.
  const waiting = row.kind === "agent" && !!home.machines?.some(m => m.id === row.machine_id && m.connecting);
  const status = moving ? <span className={`chip ${moving.chip === "Moving" ? "run" : "bad"}`}>{moving.chip}</span> : closing || (row.status === "asking" ? <span className="chip wait">needs you</span> : working ? <span className="cc-running"><span className="dspin" role="img" aria-label={row.status === "starting" ? "Starting" : "Working"} />{runningWhen(row)}</span> : runningWhen(row));
  return { foot: waiting ? <span className="cc-wait">connecting</span> : status, waiting, why: moving?.why ?? null };
}

const sessionKey = (s: { machine_id: string; id: string }) => runningKey({ kind: "agent", ...s });

/// What waits on the user: questions and approvals, pull requests, sync conflicts.
function needsCards(home: HomeView): BoardCard[] {
  const cards: BoardCard[] = [];

  for (const need of home.needs_you) {
    const out = state.outcomes.get(cardKey(need)), details = needsLine(need);
    const open = () => need.kind === "chat" ? go("pane", { chat: { session: need.session_id } }) : openAsking(need);
    const act = () => need.kind === "chat" ? answerChat(need, "allow", "Allowed") : openAsking(need);
    cards.push({
      key: need.kind === "chat" ? cardKey(need) : sessionKey({ machine_id: need.machine_id, id: need.session }), lane: "needs", session: need.kind === "agent",
      lead: need.kind === "chat" ? <span className="cc-ag beep"><span style={{ display: "contents" }}><Icon name="beep" /></span></span> : <HarnessIcon agent={need.agent} />,
      title: details.title, sub: out?.error && !out.final ? <span className="err">{out.error}</span> : details.sub,
      foot: out?.said ? <span className="ok">{out.said}</span> : out?.final ? <span className="err">{out.error}</span> : null,
      action: out?.said || out?.final ? null : { label: details.button, primary: true, run: act }, dim: !!out?.said, hint: null, open, menu: null,
    });
  }

  for (const item of (state.git?.items || []).filter(waitsOnYou).sort(newestFirst)) {
    const open = () => openUrl(item.url);
    cards.push({ key: item.url, lane: "needs", session: false, lead: <span style={{ display: "contents" }}><Icon name={item.kind === "pull_request" ? "pr" : "issue"} /></span>,
      title: `#${item.number} ${item.title}`, sub: [item.repo, itemChips(item).map(c => c.text).join(", ")].filter(Boolean).join(" · "), foot: null,
      action: { label: item.review_requested ? "Review" : "Open", primary: false, run: open }, dim: false, hint: item.url, open, menu: null });
  }

  for (const project of syncProjects()) {
    const detail = fsync.detail(project);

    for (const conflict of conflictList(detail)) {
      cards.push({ key: `${project}:${conflict.path}`, lane: "needs", session: false, lead: <span style={{ display: "contents" }}><Icon name="sync" /></span>,
        title: conflict.path, sub: conflictSub(project, conflict, detail?.machines ?? []), foot: null,
        action: { label: "Resolve", primary: false, run: () => go("sync") }, dim: false, hint: conflict.path, open: () => go("sync"), menu: null });
    }
  }

  return cards;
}

/// Everything that runs, the latest first.
function runningCards(home: HomeView): BoardCard[] {
  const projects = home.projects || [];
  return byRecent(visibleRunning(home.running)).map(row => {
    const { title, sub } = row.kind === "agent" ? runningLineAt(row, projects) : runningLine(row, projects);
    const status = runningStatus(row, home);
    return { key: runningKey(row), lane: row.kind === "agent" ? laneOf(row.status) : "working", session: row.kind === "agent", lead: row.kind === "agent" ? <HarnessIcon agent={row.agent} /> : null,
      title, sub, foot: status.foot, action: null, dim: status.waiting, hint: status.why,
      open: () => row.kind === "research" ? openResearch(row.session) : go("pane", { agent: row }),
      menu: row.kind === "agent" ? event => { void sessionMenu(event, row, s => go("pane", { agent: s })); } : null };
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
    const key = sessionKey(session);
    if (shown.has(key)) continue;
    const { title, sub } = runningLine({ kind: "agent", ...session }, projects);
    cards.push({ key, lane: "done", session: true, lead: <HarnessIcon agent={session.agent} />, title, sub, foot: formatAgo(session.last_activity, "since"),
      action: null, dim: false, hint: null, open: () => go("pane", { agent: { kind: "agent", ...session } }), menu: null });
  }

  return cards;
}

/// The Done today lane keeps to its latest sessions; the Sessions list has the rest.
const DONE_SHOWN = 8;

export function HomeOverview({ home }: { home: HomeView }) {
  const list = frontLayout("home") === "list";
  const needs = needsCards(home), running = runningCards(home);
  const cards = list ? [...needs, ...running] : boardCards(home, needs, running);
  const coming = home.coming_up, updates = home.updates;
  const empty = !cards.length && !updates?.length && !coming?.length;
  return <>
    {list ? !!needs.length && <Section id="sec-needs" title="Needs you" count={needs.length}>{needs.map(card => <CardRow key={card.key} card={card}/>)}</Section>
      : !!cards.length && <div id="sec-needs"><Board cards={cards} more={{ working: () => go("pane", "agents"), done: () => go("pane", "agents") }}/></div>}
    {!!updates?.length && <Section id="sec-updates" title="Updates" count={updates.length} action={<button className="dbtn quiet small" type="button" onClick={() => dismissUpdates(updates.map(u => u.id))}>Clear</button>}>{updates.map(update => <LineRow key={update.id} lead={<span className="cc-ag beep">{update.app_name?.slice(0, 1).toUpperCase() || <span style={{ display: "contents" }}><Icon name="beep" /></span>}</span>} title={update.summary} sub={update.app_name ? `${update.app_name} · ${formatAgo(update.at, "day")}` : formatAgo(update.at, "day")} textTitle="Ask Beep about this" onOpen={() => { void openUpdate(update); }} right={<button className="dbtn quiet" type="button" aria-label="Dismiss" style={{ padding: "2px 6px" }} onClick={event => { event.stopPropagation(); dismissUpdates([update.id]); }}><Icon name="close" /></button>} />)}</Section>}
    {list && !!running.length && <Section id="sec-running" title={<button className="dbtn lk" type="button" onClick={() => go("pane", "agents")}>Running</button>} count={running.length}>{running.map(card => <CardRow key={card.key} card={card}/>)}</Section>}
    {!!coming?.length && <Section id="sec-coming" title={<button className="dbtn lk" type="button" onClick={() => go("pane", "routines")}>Coming up</button>} count={coming.length}>{coming.slice(0, 5).map(row => { const when = whenOf(row.next_run_at); return <LineRow key={row.id} lead={<span className="cc-when"><b>{when.time}</b>{when.day}</span>} title={row.label.trim() || row.prompt.trim() || "Routine"} sub={scheduleText(row.schedule)} onOpen={() => go("pane", "routines")} />; })}</Section>}
    {empty && <div className="cc-panel cc-empty">{!state.subscribed ? <><b>{!state.home && !S.server ? "Start an agent" : "Nothing running"}</b><span>{!state.home && !S.server ? "Your machines connect in the background." : "Start an agent on this computer, or add a machine to run it there."}</span><button type="button" className="dbtn small" onClick={() => go("pane", { agentnew: { prompt: "" } })}>Start an agent</button></> : <span>Nothing needs you, and nothing is running.</span>}</div>}
  </>;
}
