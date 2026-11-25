import { HarnessIcon } from "../ui/icons.tsx";
import { EmptyPanel, LineRow, SectionHead } from "../ui/kit.tsx";
import { RunningStatus } from "./split-view.tsx";
import { byRecent, runningKey, type RunningAgent } from "../agents/running-state.ts";
import { runningLineAt } from "../agents/place.ts";
import { moveLabel } from "../agents/move-store.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { state } from "../center/state.ts";
import { go } from "../center/nav.ts";

export function ProjectRunning({ running }: { running: RunningAgent[] }) {
  return <><SectionHead title="Agents" count={running.length} />{running.length ? <div className="cc-panel">{byRecent(running).map(row => {
    const { title, sub } = runningLineAt(row, state.home?.projects || []);
    const moving = moveLabel(row);
    const open = () => go("pane", { agent: row });
    return <LineRow key={runningKey(row)} dataKey={runningKey(row)} lead={<HarnessIcon agent={row.agent} />} title={title} sub={sub} right={<RunningStatus row={row} />} rowTitle={moving?.why || undefined} onOpen={open} onContext={event => void sessionMenu(event, row, selected => go("pane", { agent: selected }))} />;
  })}</div> : <EmptyPanel text="No agent is working in this project right now." />}</>;
}
