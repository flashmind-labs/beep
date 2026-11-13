import { memo, type MouseEvent } from "react";
import { useStore } from "zustand";
import { runningRow, type RunningProcess } from "./running-state.ts";
import { backgroundTasks, paneTasks, runningModel, type BackgroundTasks } from "./running.ts";
import { Button } from "../ui/kit.tsx";

export interface RunningModel { processes: readonly RunningProcess[]; local: boolean; stopping: ReadonlySet<number>; whole: ReadonlySet<number>; machine: string; error: string }
export interface RunningActions { open(url: string): void; toggle(pid: number): void; stop(pids: number[]): void }

/// The Running tab's view in its dock.
export const RunningPane = memo(function RunningPane({ hidden }: { hidden: boolean }) {
  const shown = useStore(runningModel);
  const background = useStore(backgroundTasks);
  const tasks = paneTasks(background);
  return <div className="agpview running" hidden={hidden}>{(shown || tasks.length > 0) && <RunningView model={shown?.model ?? null} actions={shown?.actions ?? null} tasks={tasks} background={background} />}</div>;
});

interface RunningViewProps { model: RunningModel | null; actions: RunningActions | null; tasks: BackgroundTasks["tasks"]; background: BackgroundTasks }

function RunningView({ model, actions, tasks, background }: RunningViewProps) {
  const processes = model?.processes ?? [];
  const open = (event: MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    event.stopPropagation();
    actions?.open(event.currentTarget.href);
  };
  // A click shows the whole command; selecting text in it does not fold it.
  const toggle = (pid: number) => { if (model && actions && !getSelection()?.toString()) actions.toggle(pid); };

  return <><div className="agrunninglist">{tasks.map(task => {
    const what = task.description || task.name, busy = background.stopping.has(task.id);
    return <div className="agart agrunningrow agtaskrow" key={task.id}><span className="t" title={what}><b>{task.name || what}</b><small>Background command</small></span><span className="r">{task.can_stop && <Button data-agtask={task.id} disabled={busy} onClick={() => background.stop(task.id)}>{busy ? "Stopping…" : "Stop"}</Button>}</span></div>;
  })}{model && actions && processes.map(process => {
    const row = runningRow(process, model.local), expanded = model.whole.has(process.pid), busy = model.stopping.has(process.pid);
    return <div className={`agart agrunningrow${expanded ? " whole" : ""}`} key={process.pid}><span className="t" data-whole={process.pid} title={expanded ? "Click to fold" : "Click to show the whole command"} onClick={() => toggle(process.pid)}><b>{row.command}</b><small>{row.detail}</small></span><span className="r">{row.ports.map(port => port.url ? <a data-port href={port.url} key={port.label} onClick={open}>{port.label}</a> : <span key={port.label}>{port.label}</span>)}<Button data-pid={process.pid} disabled={busy} onClick={() => actions.stop([process.pid])}>{busy ? "Stopping…" : "Stop"}</Button></span></div>;
  })}</div>{model && actions && processes.length > 0 && <div className="agpacts"><span className="meta">{model.machine} · closing the session stops these</span><span className="sp" /><Button danger data-stop-all disabled={model.stopping.size > 0} onClick={() => actions.stop(processes.map(p => p.pid))}>Stop all</Button></div>}{model && <div className="derr agrunningerror" role="status" hidden={!model.error}>{model.error}</div>}</>;
}
