import type { ReactNode } from "react";
import { S } from "../bar/core.ts";
import { visibleRunning } from "../agents/closing.ts";
import { closingNote } from "../agents/closing.ts";
import { moveLabel } from "../agents/move-store.ts";
import { runningLineAt } from "../agents/place.ts";
import { byRecent, runningKey, runningLine, runningWhen, type RunningRow } from "../agents/running-state.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { conflictList, conflictSub } from "../project/filesync.ts";
import { fsync } from "../project/sections.ts";
import { syncProjects } from "../project/syncpage.ts";
import { HarnessIcon, Icon } from "../ui/icons.tsx";
import { type HomeView, needsLine } from "./home-state.ts";
import { answerChat, cardKey, dismissUpdates, openAsking, openUpdate, scheduleText, whenOf } from "./home.ts";
import { itemChips, newestFirst, waitsOnYou } from "./openitems-state.ts";
import { openUrl } from "./openitems.ts";
import { state } from "./state.ts";
import { go, openResearch } from "./views.ts";
import { formatAgo } from "../ui/format.ts";
import { machinesReach } from "../machines/machine-state.ts";
import { LineRow, Section } from "../ui/kit.tsx";

function Action({ label, action, className = "small" }: { label: ReactNode; action(): void; className?: string }) {
  return <button type="button" className={`dbtn ${className}`} onClick={event => { event.stopPropagation(); action(); }}>{label}</button>;
}
function Running({ row, home }: { row: RunningRow; home: HomeView }) {
  const { title, sub } = row.kind === "agent" ? runningLineAt(row, home.projects || []) : runningLine(row, home.projects || []);
  const moving = row.kind === "agent" ? moveLabel(row) : null;
  const closing = row.kind === "agent" ? closingNote(row) : null;
  const working = row.kind === "agent" && ["working", "running", "starting"].includes(row.status);
  const status = moving ? <span className={`chip ${moving.chip === "Moving" ? "run" : "bad"}`}>{moving.chip}</span> : closing || (row.status === "asking" ? <span className="chip wait">needs you</span> : working ? <span className="cc-running"><span className="dspin" role="img" aria-label={row.status === "starting" ? "Starting" : "Working"} />{runningWhen(row)}</span> : runningWhen(row));
  const open = () => row.kind === "research" ? openResearch(row.session) : go("pane", { agent: row });
  // Shown as last seen until its machine answers.
  const waiting = row.kind === "agent" && !!home.machines?.some(m => m.id === row.machine_id && m.connecting);
  return <LineRow lead={row.kind === "agent" ? <HarnessIcon agent={row.agent} /> : null} title={title} sub={sub} right={waiting ? <span className="cc-wait">connecting</span> : status} dim={waiting} onOpen={open} rowTitle={moving?.why || undefined} onContext={row.kind === "agent" ? event => { void sessionMenu(event, row, s => go("pane", { agent: s })); } : undefined} />;
}

export function HomeOverview({ home }: { home: HomeView }) {
  const prs = (state.git?.items || []).filter(waitsOnYou).sort(newestFirst);
  const conflicts = syncProjects().flatMap(project => { const detail = fsync.detail(project); return conflictList(detail).map(conflict => ({ project, conflict, sub: conflictSub(project, conflict, detail?.machines ?? []) })); });
  const waiting = home.needs_you.length + prs.length + conflicts.length;
  const running = visibleRunning(home.running), coming = home.coming_up, updates = home.updates;
  const empty = !waiting && !updates?.length && !running?.length && !coming?.length;
  return <>
    {!!waiting && <Section id="sec-needs" title="Needs you" count={waiting}>{home.needs_you.map(need => {
      const out = state.outcomes.get(cardKey(need)), details = needsLine(need);
      const open = () => need.kind === "chat" ? go("pane", { chat: { session: need.session_id } }) : openAsking(need);
      const act = () => need.kind === "chat" ? answerChat(need, "allow", "Allowed") : openAsking(need);
      const right = out?.said ? <span className="ok">{out.said}</span> : out?.final ? <span className="err">{out.error}</span> : <Action label={details.button} className="pri small" action={act} />;
      return <LineRow key={cardKey(need)} lead={need.kind === "chat" ? <span className="cc-ag beep"><span style={{ display: "contents" }}><Icon name="beep" /></span></span> : <HarnessIcon agent={need.agent} />} title={details.title} sub={out?.error && !out.final ? <span className="err">{out.error}</span> : details.sub} right={right} onOpen={open} dim={!!out?.said} />;
    })}{prs.map(item => { const open = () => openUrl(item.url); return <LineRow key={`${item.url}`} lead={<span style={{ display: "contents" }}><Icon name={item.kind === "pull_request" ? "pr" : "issue"} /></span>} title={`#${item.number} ${item.title}`} sub={[item.repo, itemChips(item).map(c => c.text).join(", ")].filter(Boolean).join(" · ")} right={<Action label={item.review_requested ? "Review" : "Open"} action={open} />} onOpen={open} rowTitle={item.url} />; })}{conflicts.map(({ project, conflict, sub }) => <LineRow key={`${project}:${conflict.path}`} lead={<span style={{ display: "contents" }}><Icon name="sync" /></span>} title={conflict.path} sub={sub} right={<Action label="Resolve" action={() => go("sync")} />} onOpen={() => go("sync")} rowTitle={conflict.path} />)}</Section>}
    {!!updates?.length && <Section id="sec-updates" title="Updates" count={updates.length} action={<button className="dbtn quiet small" type="button" onClick={() => dismissUpdates(updates.map(u => u.id))}>Clear</button>}>{updates.map(update => <LineRow key={update.id} lead={<span className="cc-ag beep">{update.app_name?.slice(0, 1).toUpperCase() || <span style={{ display: "contents" }}><Icon name="beep" /></span>}</span>} title={update.summary} sub={update.app_name ? `${update.app_name} · ${formatAgo(update.at, "day")}` : formatAgo(update.at, "day")} textTitle="Ask Beep about this" onOpen={() => { void openUpdate(update); }} right={<button className="dbtn quiet" type="button" aria-label="Dismiss" style={{ padding: "2px 6px" }} onClick={event => { event.stopPropagation(); dismissUpdates([update.id]); }}><Icon name="close" /></button>} />)}</Section>}
    {!!running?.length && <Section id="sec-running" title={<button className="dbtn lk" type="button" onClick={() => go("pane", "agents")}>Running</button>} count={running.length}>{byRecent(running).map(row => <Running key={runningKey(row)} row={row} home={home} />)}</Section>}
    {!!coming?.length && <Section id="sec-coming" title={<button className="dbtn lk" type="button" onClick={() => go("pane", "routines")}>Coming up</button>} count={coming.length}>{coming.slice(0, 5).map(row => { const when = whenOf(row.next_run_at); return <LineRow key={row.id} lead={<span className="cc-when"><b>{when.time}</b>{when.day}</span>} title={row.label.trim() || row.prompt.trim() || "Routine"} sub={scheduleText(row.schedule)} onOpen={() => go("pane", "routines")} />; })}</Section>}
    {empty && <Empty home={home} />}
  </>;
}

/// Nothing to show: nothing runs, or no machine has answered yet.
function Empty({ home }: { home: HomeView }) {
  const reach = machinesReach(home.machines ?? []);
  if (reach !== "reached") return <div className="cc-panel cc-empty"><span>{reach === "connecting" ? "Connecting to your machines…" : "No machine is online."}</span></div>;
  return <div className="cc-panel cc-empty">{!state.subscribed ? <><b>{!state.home && !S.server ? "Start an agent" : "Nothing running"}</b><span>{!state.home && !S.server ? "Your machines connect in the background." : "Start an agent on this computer, or add a machine to run it there."}</span><button type="button" className="dbtn small" onClick={() => go("pane", { agentnew: { prompt: "" } })}>Start an agent</button></> : <span>Nothing needs you, and nothing is running.</span>}</div>;
}
