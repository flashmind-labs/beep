import { useLayoutEffect, useRef } from "react";
import { useStore } from "zustand";
import { Button, EndedFold, Spinner } from "../../ui/kit.tsx";
import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { newChat } from "../../stores/bar/core.ts";
import { AGENT_ICON, agentsList, listedMenu, openListed, toggleEnded, type ListedSession } from "./agents-list.ts";
import { HarnessIcon } from "../../ui/icons.tsx";
import { SessionRowSlot } from "../plugins/slot-view.tsx";
import type { MachinesReach } from "../machines/machine-state.ts";

export interface AgentListRow { key: string; depth: number; agent: string; title: string; sub: string; tail: { kind: "review" } | { kind: "move"; label: string; why: string | null } | { kind: "asking" } | { kind: "running"; doing: string } | { kind: "plain"; label: string } | { kind: "connecting" } }

/// Every coding agent session: the live ones, then the ended ones folded
/// under their own group (closed until opened).
export function AgentsList({ hints }: { hints: HintViewModel }) {
  const list = useStore(agentsList);
  const selected = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { selected.current?.scrollIntoView({ block: "nearest" }); }, [list.selected, list.endedOpen]);

  const any = list.live.length + list.ended.length > 0;
  const row = (entry: ListedSession, index: number, gone: boolean) => <SessionRow key={entry.row.key} index={index} entry={entry} gone={gone} picked={index === list.selected} pickedRef={index === list.selected ? selected : null} />;

  return <><div className="dhead"><span className="tt"><b>Coding agents</b><small>{list.summary}</small></span><Button id="agNew" onClick={newChat}><Icon />New<kbd>⌘⇧A</kbd></Button></div>
    {list.error && <div className="derr dpadx">{list.error}</div>}
    <div className="dagscroll">
      {any && <div className="dlist top" role="listbox" aria-label="Coding agents">
        {list.live.length ? list.live.map((entry, index) => row(entry, index, false)) : <div className="dagquiet">Nothing running. Start one with New.</div>}
        {list.ended.length > 0 && <EndedFold count={list.ended.length} open={list.endedOpen} onToggle={toggleEnded} />}
        {list.endedOpen && list.ended.map((entry, index) => row(entry, list.live.length + index, true))}
      </div>}
    </div>
    {!any && !list.loading && <Empty reach={list.reach} />}
    <HintView value={hints} />
  </>;
}

/// No session listed: none runs, or no machine has answered yet.
function Empty({ reach }: { reach: MachinesReach }) {
  switch (reach) {
    case "connecting": return <div className="dempty"><span className="ddot dial" /><b className="dbig">Connecting to your machines…</b></div>;
    case "offline": return <div className="dempty"><span className="ddot off" /><b className="dbig">No machine is online.</b></div>;
    case "reached": return <div className="dempty"><span className="dicon"><Icon /></span><b className="dbig">No sessions running.</b><span className="dmuted dblock">Start one here, or run Claude Code in a terminal on a linked machine.</span></div>;
  }
}

function SessionRow({ index, entry, gone, picked, pickedRef }: { index: number; entry: ListedSession; gone: boolean; picked: boolean; pickedRef: React.RefObject<HTMLButtonElement | null> | null }) {
  const { row, session } = entry;
  const cls = ["dchat", row.depth ? "kid" : "", gone ? "gone" : "", row.tail.kind === "connecting" ? "wait" : ""].filter(Boolean).join(" ");
  return <button ref={pickedRef} className={cls} style={row.depth ? { "--depth": row.depth } as React.CSSProperties : undefined} role="option" aria-selected={picked} data-agi={index}
    onClick={() => openListed(session)} onContextMenu={e => listedMenu(e.nativeEvent, session)}>
    <HarnessIcon agent={row.agent} className="dapp dcoderav" />
    <span className="t"><span>{row.title}</span><small>{row.sub}</small></span>
    <SessionRowSlot machine={session.machine_id} session={session.id} title={row.title} />
    <Tail tail={row.tail} />
  </button>;
}

function Tail({ tail }: { tail: AgentListRow["tail"] }) {
  switch (tail.kind) {
    case "review": return <span className="w">Reviewing</span>;
    case "move": return <span className={`dstate ${tail.label === "Moving" ? "run" : "ask"}`} title={tail.why ?? undefined}>{tail.label}</span>;
    case "asking": return <span className="dstate ask">Needs you</span>;
    case "connecting": return <span className="w" title="Shown as last seen until its machine answers">Connecting</span>;
    case "running": return <span className="run" title={tail.doing}><Spinner dot /><span className="tx">{tail.doing}</span></span>;
    case "plain": return <span className="w">{tail.label}</span>;
  }
}

const Icon = () => <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: AGENT_ICON }} />;

