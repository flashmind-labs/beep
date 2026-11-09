import { useLayoutEffect, useRef } from "react";
import { patchChildren } from "../ui/patch.ts";
import { SyncMachine, SyncProject } from "./syncpage-view.tsx";
import type { makeFileSync, MachineDetail } from "./filesync.ts";

type Sync = ReturnType<typeof makeFileSync>;
export interface ProjectMachineItem {
  id: string; name: string; online: boolean; folder: string | null; actions: HTMLElement[]; missing: HTMLElement[];
  git: HTMLElement[]; terminals: { id: string; name: string; title: string; sub: string; note: string; running: boolean; open(): void }[]; terminalSummary: string; menu: ((event: MouseEvent) => void) | null;
}
function Legacy({ nodes }: { nodes: HTMLElement[] }) {
  const host = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => { if (host.current) patchChildren(host.current, nodes); }, [nodes]);
  return <span ref={host} style={{ display: "contents" }} />;
}
function Machine({ item, sync, project, detail, available }: { item: ProjectMachineItem; sync: Sync; project: string; detail: MachineDetail | null; available: boolean }) {
  return <article className="pm-machine" aria-label={item.name}><div className="pm-head" onContextMenu={item.menu ? event => item.menu?.(event.nativeEvent) : undefined}><span className={`ddot${item.online ? "" : " off"}`} /><b>{item.name}</b>{item.folder ? <code className="pm-folder" title={item.folder}>{item.folder}</code> : <Legacy nodes={item.missing} />}<span className="pm-actions"><Legacy nodes={item.actions} /></span></div>
    {item.folder && <div className="pm-body">{detail ? <SyncMachine sync={sync} project={project} machine={detail} compact extra={<Legacy nodes={item.git} />} /> : <div className="fs-row pm-sync"><div className="fs-m">{item.online ? available ? "Reading sync state…" : "Sync details unavailable" : "Machine offline"}</div><Legacy nodes={item.git} /></div>}<hr /><div className="pm-terminal-head"><span>Terminals</span><small>{item.terminalSummary}</small></div>{item.terminals.map(terminal => <div key={terminal.id} className="cc-row cc-line" title={terminal.title} onClick={terminal.open}><span className={`ddot${terminal.running ? "" : " off"}`} /><button type="button" className="t" title={terminal.name}><b>{terminal.name}</b><span>{terminal.sub}</span></button><span className="r">{terminal.note}</span></div>)}</div>}
  </article>;
}
export function MachinesSection({ sync, project, items, model, imported, known, count, fresh }: { sync: Sync; project: string; items: ProjectMachineItem[]; model: ReturnType<Sync["page"]> | null; imported: boolean; known: boolean; count: string; fresh: HTMLElement[] }) {
  const machines = <div className="pm-list">{items.length ? items.map(item => <Machine key={item.id} item={item} sync={sync} project={project} detail={model?.detail?.machines.find(machine => machine.machine_id === item.id) ?? null} available={model?.detail !== null} />) : <div className="cc-panel cc-empty"><span>No machines have this project.</span></div>}</div>;
  return <>{imported && model ? <SyncProject sync={sync} project={project} heading="Machines" model={model} machines={machines} /> : machines}{!known && <small>on {count}</small>}<Legacy nodes={fresh} /></>;
}
