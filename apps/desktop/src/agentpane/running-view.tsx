import { runningRow, type RunningProcess } from "./running-state.ts";

export function RunningView({ processes, local, stopping, whole, machine, error }: { processes: readonly RunningProcess[]; local: boolean; stopping: ReadonlySet<number>; whole: ReadonlySet<number>; machine: string; error: string }) {
  return <><div className="agrunninglist">{processes.map(process => {
    const row = runningRow(process, local), expanded = whole.has(process.pid), busy = stopping.has(process.pid);
    return <div className={`agart agrunningrow${expanded ? " whole" : ""}`} key={process.pid}><span className="t" data-whole={process.pid} title={expanded ? "Click to fold" : "Click to show the whole command"}><b>{row.command}</b><small>{row.detail}</small></span><span className="r">{row.ports.map(port => port.url ? <a data-port href={port.url} key={port.label}>{port.label}</a> : <span key={port.label}>{port.label}</span>)}<button type="button" className="dbtn" data-pid={process.pid} disabled={busy}>{busy ? "Stopping…" : "Stop"}</button></span></div>;
  })}</div><div className="agpacts"><span className="meta">{machine} · closing the session stops these</span><span className="sp" /><button type="button" className="dbtn warn" data-stop-all disabled={stopping.size > 0}>Stop all</button></div><div className="derr agrunningerror" role="status" hidden={!error}>{error}</div></>;
}
