import { HarnessIcon } from "../ui/icons.tsx";
import { Chip, EmptyPanel, LineRow, SectionHead, Spinner } from "../ui/kit.tsx";
import { byRecent, runningKey, runningWhen, type RunningAgent } from "../agents/running-state.ts";
import { runningLineAt } from "../agents/place.ts";
import { moveLabel } from "../agents/move-store.ts";
import { closingNote } from "../agents/closing.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { state } from "../center/state.ts";
import { go } from "../center/views.ts";

export function ProjectRunning({ running }: { running: RunningAgent[] }) {
  return <><SectionHead title="Agents" count={running.length} />{running.length ? <div className="cc-panel">{byRecent(running).map(row => {
    const { title, sub } = runningLineAt(row, state.home?.projects || []);
    const moving = moveLabel(row), closing = closingNote(row);
    const working = ["working", "running", "starting"].includes(row.status) && !moving && !closing;
    const open = () => go("pane", { agent: row });
    const status = moving ? <Chip kind={moving.chip === "Moving" ? "run" : "bad"} text={moving.chip} />
      : closing || (row.status === "asking" ? <Chip kind="wait" text="needs you" /> : working ? <span className="cc-running"><Spinner label={row.status === "starting" ? "Starting" : "Working"} />{runningWhen(row)}</span> : runningWhen(row));
    return <LineRow key={runningKey(row)} dataKey={runningKey(row)} lead={<HarnessIcon agent={row.agent} />} title={title} sub={sub} right={status} rowTitle={moving?.why || undefined} onOpen={open} onContext={event => void sessionMenu(event, row, selected => go("pane", { agent: selected }))} />;
  })}</div> : <EmptyPanel text="No agent is working in this project right now." />}</>;
}
