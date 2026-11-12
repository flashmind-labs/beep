// A project's board: its sessions by lane, with the pull requests, sync
// conflicts and questions that wait on the user.
import { byRecent, runningKey, runningLine, type RunningAgent } from "../agents/running-state.ts";
import { runningLineAt } from "../agents/place.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { Board, type BoardCard } from "../center/lanes-view.tsx";
import { endedToday, laneOf, type Lane } from "../center/lanes-state.ts";
import { state } from "../center/state.ts";
import { go } from "../center/views.ts";
import { formatAgo } from "../ui/format.ts";
import { HarnessIcon, Icon } from "../ui/icons.tsx";
import { conflictList, conflictSub } from "./filesync.ts";
import type { SessionRow } from "./projects-state.ts";
import { fsync } from "./sections.ts";
import { RunningStatus } from "./split-view.tsx";

/// A pull request that has a lane: one that needs a reply, or is ready to review.
export interface PullCard { key: string; lane: Lane; number: number; title: string; sub: string }

export function ProjectBoard({ project, running, pulls, past, openTile }: {
  project: string; running: readonly RunningAgent[]; pulls: readonly PullCard[]; past: readonly SessionRow[]; openTile(key: string): void;
}) {
  const projects = state.home?.projects || [];
  const cards: BoardCard[] = [];

  for (const row of byRecent(running)) {
    const lane = laneOf(row.status), open = () => go("pane", { agent: row });
    cards.push({ key: runningKey(row), lane, project, session: true, lead: <HarnessIcon agent={row.agent}/>, ...runningLineAt(row, projects), foot: <RunningStatus row={row}/>,
      action: lane === "needs" ? { kind: "run", label: "Answer", primary: true, run: open } : null, dim: false, hint: null, open,
      menu: event => { void sessionMenu(event, row, session => go("pane", { agent: session })); } });
  }

  for (const pull of pulls) {
    const open = () => openTile(pull.key);
    cards.push({ key: pull.key, lane: pull.lane, project, session: false, lead: <span style={{ display: "contents" }}><Icon name="pr"/></span>, title: `#${pull.number} ${pull.title}`, sub: pull.sub, foot: null,
      action: { kind: "run", label: pull.lane === "needs" ? "Reply" : "Review", primary: false, run: open }, dim: false, hint: null, open, menu: null });
  }

  const detail = fsync.detail(project);

  for (const conflict of conflictList(detail)) {
    cards.push({ key: `sync:${conflict.path}`, lane: "needs", project, session: false, lead: <span style={{ display: "contents" }}><Icon name="sync"/></span>, title: conflict.path,
      sub: conflictSub(null, conflict, detail?.machines ?? []), foot: null, action: { kind: "run", label: "Resolve", primary: false, run: () => go("sync") }, dim: false, hint: conflict.path, open: () => go("sync"), menu: null });
  }

  for (const session of endedToday(past, Date.now())) {
    cards.push({ key: runningKey({ kind: "agent", ...session }), lane: "done", project, session: true, lead: <HarnessIcon agent={session.agent}/>, ...runningLine({ kind: "agent", ...session }, projects),
      foot: formatAgo(session.last_activity, "since"), action: null, dim: false, hint: null, open: () => go("pane", { agent: { kind: "agent", ...session } }), menu: null });
  }

  return <Board cards={cards} more={{ working: () => openTile("running"), done: () => openTile("agents") }} rows={null}/>;
}
