import type { MouseEvent } from "react";
import { useStore } from "zustand";
import { runningRow, type RunningProcess } from "./running-state.ts";
import { runningModel } from "./running.ts";

export interface RunningModel { processes: readonly RunningProcess[]; local: boolean; stopping: ReadonlySet<number>; whole: ReadonlySet<number>; machine: string; error: string }
export interface RunningActions { open(url: string): void; toggle(pid: number): void; stop(pids: number[]): void }

/// The Running tab's view in its dock.
export function RunningPane({ hidden }: { hidden: boolean }) {
  const shown = useStore(runningModel);
  return <div className="agpview running" hidden={hidden}>{shown && <RunningView model={shown.model} actions={shown.actions} />}</div>;
}

function RunningView({ model, actions }: { model: RunningModel; actions: RunningActions }) {
  const { processes, local, stopping, whole, machine, error } = model;
  const open = (event: MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    event.stopPropagation();
    actions.open(event.currentTarget.href);
  };
  // A click shows the whole command; selecting text in it does not fold it.
  const toggle = (pid: number) => { if (!getSelection()?.toString()) actions.toggle(pid); };

  return <><div className="agrunninglist">{processes.map(process => {
    const row = runningRow(process, local), expanded = whole.has(process.pid), busy = stopping.has(process.pid);
    return <div className={`agart agrunningrow${expanded ? " whole" : ""}`} key={process.pid}><span className="t" data-whole={process.pid} title={expanded ? "Click to fold" : "Click to show the whole command"} onClick={() => toggle(process.pid)}><b>{row.command}</b><small>{row.detail}</small></span><span className="r">{row.ports.map(port => port.url ? <a data-port href={port.url} key={port.label} onClick={open}>{port.label}</a> : <span key={port.label}>{port.label}</span>)}<button type="button" className="dbtn" data-pid={process.pid} disabled={busy} onClick={() => actions.stop([process.pid])}>{busy ? "Stopping…" : "Stop"}</button></span></div>;
  })}</div><div className="agpacts"><span className="meta">{machine} · closing the session stops these</span><span className="sp" /><button type="button" className="dbtn warn" data-stop-all disabled={stopping.size > 0} onClick={() => actions.stop(processes.map(p => p.pid))}>Stop all</button></div><div className="derr agrunningerror" role="status" hidden={!error}>{error}</div></>;
}
