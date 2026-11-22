import { memo, type MouseEvent } from "react";
import { useStore } from "zustand";
import { runningRow, runningTrees, shownCommand, type RunningProcess } from "./running-state.ts";
import { backgroundTasks, paneTasks, runningModel, type BackgroundTasks } from "./running.ts";
import { Button } from "../ui/kit.tsx";

export interface RunningModel { processes: readonly RunningProcess[]; stopping: ReadonlySet<number>; whole: ReadonlySet<number>; machine: string; error: string }
/// `open` shows a port of the session's machine in the browser, forwarded here first when the machine is another one.
export interface RunningActions { open(port: number): void; toggle(pid: number): void; stop(pids: number[]): void }

/// The Running tab's view in its dock.
export const RunningPane = memo(function RunningPane({ hidden }: { hidden: boolean }) {
  const shown = useStore(runningModel);
  const background = useStore(backgroundTasks);
  const tasks = paneTasks(background);
  return <div className="agpview running" hidden={hidden}>{(shown || tasks.length > 0) && <RunningView model={shown?.model ?? null} actions={shown?.actions ?? null} tasks={tasks} background={background} />}</div>;
});

/// "Open :8788" above the field: the pages the session's servers show, while its Running list is read.
export function RunningPages() {
  const shown = useStore(runningModel);
  const ports = shown ? runningTrees(shown.model.processes).flatMap(tree => runningRow(tree).page?.port ?? []) : [];
  if (!shown || ports.length === 0) return null;
  return <div className="ag-tasks ag-page">{ports.map(port => <Button look="plain" key={port} title="Open in the browser" onClick={() => shown.actions.open(port)}>{`Open :${port}`}</Button>)}</div>;
}

interface RunningViewProps { model: RunningModel | null; actions: RunningActions | null; tasks: BackgroundTasks["tasks"]; background: BackgroundTasks }

function RunningView({ model, actions, tasks, background }: RunningViewProps) {
  const processes = model?.processes ?? [];
  const open = (event: MouseEvent<HTMLAnchorElement>, port: number) => {
    event.preventDefault();
    event.stopPropagation();
    actions?.open(port);
  };
  // A click shows the whole command; selecting text in it does not fold it.
  const toggle = (pid: number) => { if (model && actions && !getSelection()?.toString()) actions.toggle(pid); };

  return <><div className="agrunninglist">{tasks.map(task => {
    const what = task.description || task.name, busy = background.stopping.has(task.id);
    return <div className="agart agrunningrow agtaskrow" key={task.id}><span className="t" title={what}><b>{task.name || what}</b><small>Background command</small></span><span className="r">{task.can_stop && <Button data-agtask={task.id} disabled={busy} onClick={() => background.stop(task.id)}>{busy ? "Stopping…" : "Stop"}</Button>}</span></div>;
  })}{model && actions && runningTrees(processes).map(tree => {
    const { root, children } = tree, pids = [root.pid, ...children.map(child => child.pid)];
    const row = runningRow(tree), expanded = model.whole.has(root.pid), busy = pids.some(pid => model.stopping.has(pid));
    const ports = row.page ? [row.page, ...(expanded ? row.ports : [])] : row.ports;
    return <div className={`agart agrunningrow${expanded ? " whole" : ""}`} key={root.pid}><span className="t" data-whole={root.pid} title={expanded ? "Click to fold" : "Click to show the whole command and its processes"} onClick={() => toggle(root.pid)}><b>{expanded ? root.command : row.command}</b><small>{row.detail}</small>{ports.length > 0 && <span className="ports">{ports.map(port => <a data-port className={port === row.page ? "page" : undefined} href={`http://localhost:${port.port}`} key={port.label} title="Open in the browser" onClick={event => open(event, port.port)}>{port.label}</a>)}</span>}{expanded && children.map(child => <small className="child" key={child.pid} title={child.command}>{child.pid} {shownCommand(child.command)}</small>)}</span><span className="r"><Button data-pid={root.pid} disabled={busy} onClick={() => actions.stop(pids)}>{busy ? "Stopping…" : "Stop"}</Button></span></div>;
  })}</div>{model && actions && processes.length > 0 && <div className="agpacts"><span className="meta">{model.machine} · closing the session stops these</span><span className="sp" /><Button danger data-stop-all disabled={model.stopping.size > 0} onClick={() => actions.stop(processes.map(p => p.pid))}>Stop all</Button></div>}{model && <div className="derr agrunningerror" role="status" hidden={!model.error}>{model.error}</div>}</>;
}
