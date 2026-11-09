import { useLayoutEffect, useRef, type ReactNode } from "react";
import { clockWarning } from "../machines/clock.ts";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.ts";
import { gitSyncWarning } from "./gitsync-state.ts";
import { conflictList, conflictSub, gitConflict, machineMeta, stateChip, stateNote, syncedBySyncthing, syncPct, syncSummary, waitingList, type Conflict, type MachineDetail, type makeFileSync } from "./filesync.ts";
import { plainPath } from "./projects-state.ts";

type Sync = ReturnType<typeof makeFileSync>;
function Legacy({ node }: { node: HTMLElement }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { const el = host.current; if (!el) return; el.append(node); return () => { node.remove(); }; }, [node]);
  return <div ref={host} style={{ display: "contents" }} />;
}
export function SyncMachine({ sync, project, machine, compact = false, extra = null }: { sync: Sync; project: string; machine: MachineDetail; compact?: boolean; extra?: ReactNode }) {
  const chip = stateChip(machine), pct = machine.paused ? null : syncPct(machine), clock = clockWarning(machine.machine_id, null), note = stateNote(machine), folder = plainPath(machine.folder);
  return <div className={compact ? "fs-row pm-sync" : "fs-row"} onContextMenu={event => { if (!compact && sync.changeFolder && machine.status !== "offline" && machine.folder) openMenu(event.nativeEvent, machine.machine_name, [{ label: "Change folder…", on: () => sync.changeFolder?.(project, machine) }]); }}>
    <div className="fs-h">{!compact && <span className={`ddot${machine.status === "offline" ? " off" : ""}`} />}<b>{compact ? machine.git_only ? "Git sync" : "File sync" : machine.machine_name}</b><span className={`chip${chip.wait ? " wait" : ""}`} title={machine.error || ""}>{chip.text}</span>{compact && sync.fixGit && !machine.paused && gitSyncWarning(machine) && <button type="button" className="dbtn small" title="Opens a draft with both checkouts and recent session references" onClick={() => sync.fixGit?.(project, machine)}>Fix with an agent</button>}{extra}</div>
    {pct !== null && <div className="fs-bar"><i style={{ width: `${pct}%` }} /></div>}{(!compact || !machine.git_only) && <div className="fs-m">{(compact ? machineMeta(machine) : machine.git_only ? ["Git sync"] : machineMeta(machine)).map((part, i) => <span key={i}>{part}</span>)}{!compact && folder && <code>{folder}</code>}</div>}
    {clock && <div className="fs-m"><span className="clockoff" title={clock.title}>{clock.text}</span></div>}{note && <div className={machine.status === "syncthing" ? "fs-m" : "cc-err"} title={machine.error || ""}>{note}</div>}
    {!compact && sync.fixGit && !machine.paused && gitSyncWarning(machine) && <div className="fs-m"><button type="button" className="dbtn small" title="Opens a draft with both checkouts and recent session references" onClick={() => sync.fixGit?.(project, machine)}>Fix with an agent</button></div>}{machine.hint && <div className="fs-m">Rescanned instead of watched. To watch it, run {machine.hint}</div>}
  </div>;
}
function ConflictRow({ sync, project, conflict, machines, busy, paused }: { sync: Sync; project: string; conflict: Conflict; machines: MachineDetail[]; busy: string | null; paused: boolean }) {
  const shown = sync.compared(project, conflict.path), comparing = shown !== undefined, disabled = !!busy || paused;
  const where = conflict.on.map(id => machines.find(machine => machine.machine_id === id)?.machine_name || "").filter(Boolean).join(", ");
  const menu = (event: MouseEvent) => {
    const items: MenuItem[] = [];
    if (!gitConflict(conflict.path)) items.push({ label: comparing ? "Hide" : "Compare", on: () => void sync.compare(project, conflict, machines) });
    if (sync.fix && conflict.holder) items.push({ label: "Fix with an agent", on: () => sync.fix?.(project, conflict) });
    if (conflict.resolver && !disabled) {
      items.push({ label: "Keep this copy…", warn: true, on: () => confirmSheet({ title: "Keep this copy?", text: `It takes the place of ${conflict.original} on every machine.`, action: "Keep this copy", run: () => sync.resolve(project, conflict, true) }) },
        { label: "Keep the original…", warn: true, on: () => confirmSheet({ title: "Keep the original?", text: `${conflict.original} stays and this copy is deleted on every machine.`, action: "Keep the original", run: () => sync.resolve(project, conflict, false) }) });
    }
    if (items.length) openMenu(event, conflict.path, items);
  };
  return <div className={`fs-conflict${comparing ? " open" : ""}`} onContextMenu={event => menu(event.nativeEvent)}><span className="t"><code>{conflict.path}</code><span>{conflictSub(null, conflict, machines)}</span></span><span className="acts">
    {!gitConflict(conflict.path) && <button type="button" className="dbtn small" title={`Shows what differs between ${conflict.original} and this copy`} onClick={() => void sync.compare(project, conflict, machines)}>{comparing ? "Hide" : "Compare"}</button>}
    {sync.fix && conflict.holder && <button type="button" className="dbtn small pri" title="Fills the bar to merge the two; you send it" onClick={() => sync.fix?.(project, conflict)}>Fix with an agent</button>}
    {conflict.resolver ? <><button type="button" className="dbtn small" disabled={disabled} title={`Puts this copy in place of ${conflict.original} on every machine`} onClick={() => void sync.resolve(project, conflict, true)}>{busy === conflict.path ? "Keeping…" : "Keep this copy"}</button><button type="button" className="dbtn small" disabled={disabled} title={`Keeps ${conflict.original} and deletes this copy on every machine`} onClick={() => void sync.resolve(project, conflict, false)}>Keep the original</button></> : <span className="dmuted">Update beep-node on a machine that has it to settle it here</span>}</span>
    {comparing && <div className="fs-cmp"><div className="fs-cmph"><span>Kept <b>{conflict.original}</b></span><span>Copy <b>{conflict.path}</b></span><small>{where ? `on ${where}` : ""}</small></div>{shown === null ? <div className="dmuted fs-cmpn">Reading…</div> : typeof shown === "string" ? <div className="dmuted fs-cmpn">{shown}</div> : shown && <Legacy node={shown} />}</div>}
  </div>;
}
export function SyncProject({ sync, project, model, machines = null, heading = project }: { sync: Sync; project: string; model: ReturnType<Sync["page"]>; machines?: ReactNode; heading?: string }) {
  const { detail, error, busy } = model;
  if (!detail) return <>{error && <div className="cc-err">{error}</div>}{machines}<div className="cc-panel cc-empty">{detail === null ? <b>The server needs an update before it shows file sync in detail.</b> : <span>Loading…</span>}</div></>;
  const conflicts = conflictList(detail), waiting = waitingList(detail);
  return <>{error && <div className="cc-err">{error}</div>}<div className="cc-sec"><h4>{heading}</h4><span className="fs-tools"><small>{syncSummary(detail)}</small><button type="button" className={`dbtn${detail.paused ? " pri" : ""}`} disabled={!!busy || detail.pinned || !detail.machines.length} hidden={!detail.paused && detail.machines.every(syncedBySyncthing)} title={detail.paused ? "Sync again on every machine" : "Hold sync on every machine; files stay as they are"} onClick={() => void sync.pause(project, !detail.paused)}>{busy === "pause" ? (detail.paused ? "Resuming…" : "Pausing…") : detail.paused ? "Resume" : "Pause"}</button></span></div>
    {machines ?? <div className="cc-panel fs-list">{detail.machines.length ? detail.machines.map(machine => <SyncMachine key={machine.machine_id} sync={sync} project={project} machine={machine} />) : <div className="cc-empty"><span>{detail.pinned ? "Pinned: never syncs." : "No machine syncs this project."}</span></div>}</div>}
    {!!detail.ignored_folders?.length && <div className="fs-m">File sync for ignored folders: {detail.ignored_folders.join(", ")}</div>}
    {!!conflicts.length && <><div className="cc-sec"><h4>Conflicts</h4><small>edited on two machines at once</small></div><div className="cc-panel fs-list">{conflicts.map(conflict => <ConflictRow key={conflict.path} sync={sync} project={project} conflict={conflict} machines={detail.machines} busy={busy} paused={detail.paused} />)}</div></>}
    {!!waiting.length && <><div className="cc-sec"><h4>Waiting</h4></div><div className="cc-panel fs-list">{waiting.map(item => <div className="fs-wait" key={item.path}><code>{item.path}</code><span className="dmuted">{item.machines.join(", ")}</span></div>)}</div></>}
    <div className="dmuted fs-note">{detail.machines.some(machine => machine.git_only) ? detail.ignored_folders?.length ? "Git syncs commits. Selected ignored folders use file sync. Other uncommitted edits stay local." : "Git checkouts sync commits. Uncommitted edits stay on their machine; use Bring work here under Machines for an explicit handoff." : "Builds, ignored files and links stay on each machine."}</div>
  </>;
}
