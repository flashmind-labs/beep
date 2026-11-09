import { harnessInner, harnessMark } from "../ui/harnessicon.ts";
import { byRecent, runningKey, runningWhen, type RunningAgent } from "../agents/running-state.ts";
import { runningLineAt } from "../agents/place.ts";
import { moveLabel } from "../agents/move-store.ts";
import { closingNote } from "../agents/closing.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { state } from "../center/state.ts";
import { go } from "../center/views.ts";

export function ProjectRunning({ running }: { running: RunningAgent[] }) {
  return <><div className="cc-sec"><h4>Agents{running.length > 0 && <span className="n">{running.length}</span>}</h4></div>{running.length ? <div className="cc-panel">{byRecent(running).map(row => {
    const { title, sub } = runningLineAt(row, state.home?.projects || []);
    const moving = moveLabel(row), closing = closingNote(row);
    const working = ["working", "running", "starting"].includes(row.status) && !moving && !closing;
    const mark = harnessMark(row.agent), safe = /^[a-z0-9-]+$/.test(row.agent) ? ` ${row.agent}` : "";
    const open = () => go("pane", { agent: row });
    return <div key={runningKey(row)} data-key={runningKey(row)} className="cc-row cc-line" title={moving?.why || undefined} onClick={open} onContextMenu={e => void sessionMenu(e.nativeEvent, row, selected => go("pane", { agent: selected }))}><span className={`cc-ag${safe}${mark ? " mark" : ""}`} dangerouslySetInnerHTML={{ __html: harnessInner(row.agent) }} /><button type="button" className="t" title={title}><b>{title}</b><span>{sub}</span></button><span className="r">{moving ? <span className={`chip ${moving.chip === "Moving" ? "run" : "bad"}`}>{moving.chip}</span> : closing || (row.status === "asking" ? <span className="chip wait">needs you</span> : working ? <span className="cc-running"><span className="dspin" role="img" aria-label={row.status === "starting" ? "Starting" : "Working"} />{runningWhen(row)}</span> : runningWhen(row))}</span></div>;
  })}</div> : <div className="cc-panel cc-empty"><span>No agent is working in this project right now.</span></div>}</>;
}
