import { Button, ProgressBar, Spinner } from "../../ui/kit.tsx";
import { createStore, useStore } from "zustand";
import type { MoveRow } from "../../stores/agents/move-state.ts";

export type MoveView = { kind: "none" } | { kind: "warning"; text: string } | { kind: "failed"; text: string } | { kind: "waiting"; to: string } | { kind: "progress"; to: string; cancelable: boolean; rows: MoveRow[]; pct: number | null };

type MenuProps = Parameters<typeof MoveMenu>[0];
type StatusProps = Parameters<typeof MoveStatus>[0];
/// What the open session's head and composer show of moving it, for the
/// session `key` (machine, harness and id) they were drawn for.
export const moveViews = createStore<{ key: string; menu: MenuProps | null; status: StatusProps | null }>(() => ({ key: "", menu: null, status: null }));

/// The machine in the session head: a menu to move the session when it can move.
export function MoveHere({ moveKey, machine }: { moveKey: string; machine: string }) {
  const { key, menu } = useStore(moveViews);
  return <span id="aghere">{key === moveKey && menu ? <MoveMenu {...menu} /> : machine}</span>;
}

/// The move above the composer: waiting, its steps, or why it failed.
export function MoveBox({ moveKey }: { moveKey: string }) {
  const { key, status } = useStore(moveViews);
  return <div id="agmovebox">{key === moveKey && status && <MoveStatus {...status} />}</div>;
}

/// The bar along the top of the message field while the move copies.
export function MoveBar({ moveKey }: { moveKey: string }) {
  const { key, status } = useStore(moveViews);
  const view = key === moveKey ? status?.view : null;
  return view?.kind === "progress" && view.pct !== null ? <ProgressBar pct={view.pct} label={`Moving to ${view.to}`} /> : null;
}

export function MoveStatus({ view, cancel, stop }: { view: MoveView; cancel(): void; stop(): void }) {
  if (view.kind === "none") return null;
  if (view.kind === "warning") return <div className="mvwarning" role="status">{view.text}</div>;
  if (view.kind === "failed") return <div className="mvs" role="status"><div className="h"><span className="t">{view.text}</span><span className="sp" /><Button look="quiet" small data-mvact="cancel" onClick={cancel}>Dismiss</Button></div></div>;
  if (view.kind === "waiting") return <div className="mvs bar" role="status"><span>Moving to {view.to} after this turn</span><span className="sp" /><Button small data-mvact="stop" onClick={stop}>Stop and move</Button><Button look="quiet" small data-mvact="cancel" onClick={cancel}>Cancel</Button></div>;
  return <div className="mvs" role="status"><div className="h"><b>Moving to {view.to}</b><span className="sp" />{view.cancelable && <Button look="quiet" small data-mvact="cancel" onClick={cancel}>Cancel move</Button>}</div>{view.rows.map((row, index) => <div className={`r ${row.state}`} key={index}><span className="mk">{row.state === "done" ? <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12l5 5 9-9" /></svg> : row.state === "run" ? <Spinner /> : null}</span><span>{row.text}</span></div>)}</div>;
}

function MoveMenu(p: { machine: string; moving: boolean; open: boolean; loading: boolean; targets: { id: string; name: string; note: string; enabled: boolean }[]; toggle(): void; pick(id: string): void }) {
  return <><button className="mvb" id="agmoveb" disabled={p.moving} aria-haspopup="menu" aria-expanded={p.open} title={p.moving ? "Moving" : "Move this session to another machine"} onClick={event => { event.stopPropagation(); p.toggle(); }}>{p.machine}{!p.moving && <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="M6 9l6 6 6-6" /></svg>}</button>
    {p.open && <div className="agwtm mvm" role="menu"><div className="mvh">Move to</div>{p.loading && <div className="mvh">Checking machines…</div>}{p.targets.map((target, index) => <button key={target.id} role="menuitem" data-mv={index} disabled={!target.enabled} onClick={() => p.pick(target.id)}><b>{target.name}</b><small>{target.note}</small></button>)}</div>}</>;
}
