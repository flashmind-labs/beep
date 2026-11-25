// A project's board: its sessions by lane, with the sync conflicts and
// questions that wait on the user.
import { projectWindowProps } from "../center/sidebar.tsx";
import { byRecent, runningKey, runningLine, type RunningAgent } from "../agents/running-state.ts";
import { runningLineAt } from "../agents/place.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import type { BoardCard } from "../center/lanes-view.tsx";
import { endedToday, laneOf, withoutProject } from "../center/lanes-state.ts";
import { state } from "../center/state.ts";
import { go } from "../center/nav.ts";
import { formatAgo } from "../ui/format.ts";
import { HarnessIcon, Icon } from "../ui/icons.tsx";
import { conflictList, conflictSub } from "./filesync.ts";
import type { SessionRow } from "./projects-state.ts";
import { fsync } from "./sections.ts";
import { RunningStatus } from "./split-view.tsx";
import { teamBoard, threadOpener } from "./team-view.tsx";

/// The cards on project `project`'s board, its team's with them; none leaves the page to the bar.
export function projectCards({ project, url, running, past, openTile }: {
  project: string; url: string | null; running: readonly RunningAgent[]; past: readonly SessionRow[]; openTile(key: string): void;
}) {
  const projects = state.home?.projects || [];
  const cards: BoardCard[] = [];

  for (const row of byRecent(running)) {
    const lane = laneOf(row.status), open = () => go("pane", { agent: row }), line = runningLineAt(row, projects);
    cards.push({ key: runningKey(row), lane, project, color: null, session: true, lead: <HarnessIcon agent={row.agent}/>, title: line.title, sub: withoutProject(line.sub, project),
      foot: <RunningStatus row={row} doing/>,
      action: lane === "needs" ? { kind: "run", label: "Answer", primary: true, run: open } : null, dim: false, theirs: false, hint: null, open,
      menu: event => { void sessionMenu(event, row, session => go("pane", { agent: session })); } });
  }

  const detail = fsync.detail(project);

  for (const conflict of conflictList(detail)) {
    cards.push({ windowProps: projectWindowProps(project), key: `sync:${conflict.path}`, lane: "needs", project, color: null, session: false, lead: <span style={{ display: "contents" }}><Icon name="sync"/></span>, title: conflict.path,
      sub: conflictSub(null, conflict, detail?.machines ?? []), foot: null, action: { kind: "run", label: "Resolve", primary: false, windowProps: projectWindowProps(project), run: () => openTile("machines") }, dim: false, theirs: false, hint: conflict.path, open: () => openTile("machines"), menu: null });
  }

  for (const session of endedToday(past, Date.now())) {
    cards.push({ key: runningKey(session), lane: "done", project, color: null, session: true, lead: <HarnessIcon agent={session.agent}/>, ...runningLine(session, projects),
      foot: formatAgo(session.last_activity, "since"), action: null, dim: false, theirs: false, hint: null, open: () => go("pane", { agent: { kind: "agent", ...session } }), menu: null });
  }

  return teamBoard(project, cards, threadOpener(project, url, openTile));
}
