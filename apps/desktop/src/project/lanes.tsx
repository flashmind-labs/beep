// A project's board: its sessions by lane, with the sync conflicts and
// questions that wait on the user.
import { byRecent, runningKey, runningLine, type RunningAgent } from "../agents/running-state.ts";
import { runningLineAt } from "../agents/place.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { Board, type BoardCard } from "../center/lanes-view.tsx";
import { endedToday, laneOf, withoutProject } from "../center/lanes-state.ts";
import { state } from "../center/state.ts";
import { go } from "../center/views.tsx";
import { formatAgo } from "../ui/format.ts";
import { HarnessIcon, Icon } from "../ui/icons.tsx";
import { WandererArt } from "../ui/painted-art.tsx";
import { conflictList, conflictSub } from "./filesync.ts";
import type { SessionRow } from "./projects-state.ts";
import { fsync } from "./sections.ts";
import { RunningStatus } from "./split-view.tsx";

export function ProjectBoard({ project, running, past, openTile }: {
  project: string; running: readonly RunningAgent[]; past: readonly SessionRow[]; openTile(key: string): void;
}) {
  const projects = state.home?.projects || [];
  const cards: BoardCard[] = [];

  for (const row of byRecent(running)) {
    const lane = laneOf(row.status), open = () => go("pane", { agent: row }), line = runningLineAt(row, projects);
    cards.push({ key: runningKey(row), lane, project, session: true, lead: <HarnessIcon agent={row.agent}/>, title: line.title, sub: withoutProject(line.sub, project),
      foot: <RunningStatus row={row} doing/>,
      action: lane === "needs" ? { kind: "run", label: "Answer", primary: true, run: open } : null, dim: false, hint: null, open,
      menu: event => { void sessionMenu(event, row, session => go("pane", { agent: session })); } });
  }

  const detail = fsync.detail(project);

  for (const conflict of conflictList(detail)) {
    cards.push({ key: `sync:${conflict.path}`, lane: "needs", project, session: false, lead: <span style={{ display: "contents" }}><Icon name="sync"/></span>, title: conflict.path,
      sub: conflictSub(null, conflict, detail?.machines ?? []), foot: null, action: { kind: "run", label: "Resolve", primary: false, run: () => openTile("machines") }, dim: false, hint: conflict.path, open: () => openTile("machines"), menu: null });
  }

  for (const session of endedToday(past, Date.now())) {
    cards.push({ key: runningKey(session), lane: "done", project, session: true, lead: <HarnessIcon agent={session.agent}/>, ...runningLine(session, projects),
      foot: formatAgo(session.last_activity, "since"), action: null, dim: false, hint: null, open: () => go("pane", { agent: { kind: "agent", ...session } }), menu: null });
  }

  return <Board cards={cards} more={{ working: () => openTile("running"), done: () => openTile("agents") }} rows={null} beside={<WandererArt/>}/>;
}
