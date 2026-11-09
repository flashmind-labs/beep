import type { MoveRow } from "./move-state.ts";

export type MoveView = { kind: "none" } | { kind: "warning"; text: string } | { kind: "failed"; text: string } | { kind: "waiting"; to: string } | { kind: "progress"; to: string; cancelable: boolean; rows: MoveRow[] };

export function MoveStatus({ view, cancel, stop }: { view: MoveView; cancel(): void; stop(): void }) {
  if (view.kind === "none") return null;
  if (view.kind === "warning") return <div className="mvwarning" role="status">{view.text}</div>;
  if (view.kind === "failed") return <div className="mvs" role="status"><div className="h"><span className="t">{view.text}</span><span className="sp" /><button className="dbtn quiet small" data-mvact="cancel" onClick={cancel}>Dismiss</button></div></div>;
  if (view.kind === "waiting") return <div className="mvs bar" role="status"><span>Moving to {view.to} after this turn</span><span className="sp" /><button className="dbtn small" data-mvact="stop" onClick={stop}>Stop and move</button><button className="dbtn quiet small" data-mvact="cancel" onClick={cancel}>Cancel</button></div>;
  return <div className="mvs" role="status"><div className="h"><b>Moving to {view.to}</b><span className="sp" />{view.cancelable && <button className="dbtn quiet small" data-mvact="cancel" onClick={cancel}>Cancel move</button>}</div>{view.rows.map((row, index) => <div className={`r ${row.state}`} key={index}><span className="mk">{row.state === "done" ? <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12l5 5 9-9" /></svg> : row.state === "run" ? <span className="dspin" /> : null}</span><span>{row.text}</span></div>)}</div>;
}
