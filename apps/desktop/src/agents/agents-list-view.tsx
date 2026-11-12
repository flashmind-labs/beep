import { useLayoutEffect, useRef } from "react";
import { useStore } from "zustand";
import { Spinner } from "../ui/kit.tsx";
import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { AGENT_ICON, agentsList, listedMenu, newFromList, openListed, toggleEnded, type ListedSession } from "./agents.ts";
import { HarnessIcon } from "../ui/icons.tsx";
import { actOnCopy, type CopiesModel } from "./copies.ts";

export interface AgentListRow { key: string; depth: number; agent: string; title: string; sub: string; tail: { kind: "review" } | { kind: "move"; label: string; why: string | null } | { kind: "asking" } | { kind: "running"; doing: string } | { kind: "plain"; label: string } | { kind: "connecting" } }

/// Every coding agent session: the live ones, then the ended ones folded
/// under their own group (closed until opened).
export function AgentsList({ hints }: { hints: HintViewModel }) {
  const list = useStore(agentsList);
  const selected = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { selected.current?.scrollIntoView({ block: "nearest" }); }, [list.selected, list.endedOpen]);

  const any = list.live.length + list.ended.length > 0;
  const row = (entry: ListedSession, index: number, gone: boolean) => <SessionRow key={entry.row.key} index={index} entry={entry} gone={gone} picked={index === list.selected} pickedRef={index === list.selected ? selected : null} />;

  return <><div className="dhead"><span className="tt"><b>Coding agents</b><small>{list.summary}</small></span><button className="dbtn" id="agNew" onClick={newFromList}><Icon />New<kbd>⌘⇧A</kbd></button></div>
    {list.error && <div className="derr dpadx">{list.error}</div>}
    <div className="dagscroll">
      {any && <div className="dlist top" role="listbox" aria-label="Coding agents">
        {list.live.length ? list.live.map((entry, index) => row(entry, index, false)) : <div className="dagquiet">Nothing running. Start one with New.</div>}
        {list.ended.length > 0 && <button className="dagfold" aria-expanded={list.endedOpen} onClick={toggleEnded}><span className="chev" aria-hidden>{list.endedOpen ? "▾" : "▸"}</span>Ended<span className="n">{list.ended.length}</span></button>}
        {list.endedOpen && list.ended.map((entry, index) => row(entry, list.live.length + index, true))}
      </div>}
      {list.copies && <Copies value={list.copies} />}
    </div>
    {!any && !list.copies && !list.loading && <div className="dempty"><span className="dicon"><Icon /></span><b className="dbig">No sessions running.</b><span className="dmuted dblock">Start one here, ask Beep in a chat, or run Claude Code in a terminal on a linked machine.</span></div>}
    <HintView value={hints} />
  </>;
}

function SessionRow({ index, entry, gone, picked, pickedRef }: { index: number; entry: ListedSession; gone: boolean; picked: boolean; pickedRef: React.RefObject<HTMLButtonElement | null> | null }) {
  const { row, session } = entry;
  const cls = ["dchat", row.depth ? "kid" : "", gone ? "gone" : "", row.tail.kind === "connecting" ? "wait" : ""].filter(Boolean).join(" ");
  return <button ref={pickedRef} className={cls} style={row.depth ? { "--depth": row.depth } as React.CSSProperties : undefined} role="option" aria-selected={picked} data-agi={index}
    onClick={() => openListed(session)} onContextMenu={e => listedMenu(e.nativeEvent, session)}>
    <HarnessIcon agent={row.agent} className="dapp dcoderav" />
    <span className="t"><span>{row.title}</span><small>{row.sub}</small></span>
    <Tail tail={row.tail} />
  </button>;
}

function Tail({ tail }: { tail: AgentListRow["tail"] }) {
  switch (tail.kind) {
    case "review": return <span className="w">Reviewing</span>;
    case "move": return <span className={`dstate ${tail.label === "Moving" ? "run" : "ask"}`} title={tail.why ?? undefined}>{tail.label}</span>;
    case "asking": return <span className="dstate ask">Needs you</span>;
    case "connecting": return <span className="w" title="Shown as last seen until its machine answers">Connecting</span>;
    case "running": return <span className="run" title={tail.doing}><Spinner /><span className="tx">{tail.doing}</span></span>;
    case "plain": return <span className="w">{tail.label}</span>;
  }
}

const Icon = () => <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: AGENT_ICON }} />;

/// The copies this computer keeps of other machines' sessions.
function Copies({ value }: { value: CopiesModel }) {
  return <div className="dlist dcopies">{value.error && <div className="derr dpadx">{value.error}</div>}{value.rows.map(row => <div className="dchat copy" key={row.key} data-cpk={row.key}><span className="t"><span>{row.title}</span><small>{row.sub}</small></span>{row.buttons.map(b => <button key={b.act} className={`dbtn ${b.pri ? "pri" : "quiet"} small`} data-cpact={b.act} disabled={b.busy} onClick={() => actOnCopy(row.key, b.act)}>{b.label}</button>)}</div>)}</div>;
}
