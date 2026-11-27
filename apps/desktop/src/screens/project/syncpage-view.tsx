import { useLayoutEffect, useRef, type ReactNode } from "react";
import { clockWarning } from "../../stores/machines/clock.ts";
import { confirmSheet, openMenu, type MenuItem } from "../../ui/menu.tsx";
import { gitSyncWarning } from "../../stores/project/gitsync-state.ts";
import { syncedBySyncthing } from "../../stores/project/projects-state.ts";
import { conflictList, conflictSub, gitConflict, machineMeta, stateChip, stateNote, syncPct, syncSummary, waitingList, type Conflict, type MachineDetail, type makeFileSync } from "../../parts/project/filesync.ts";
import { Button, Chip, EmptyPanel, SectionHead } from "../../ui/kit.tsx";
import { AgentButton } from "../../parts/review/feedback-view.tsx";

type Sync = ReturnType<typeof makeFileSync>;
/// The diff view of a compared conflict, a DOM tree its owner keeps.
function DiffHost({ node }: { node: HTMLElement }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { const el = host.current; if (!el) return; el.append(node); return () => { node.remove(); }; }, [node]);
  return <div ref={host} style={{ display: "contents" }} />;
}
/// The agent button that fixes a machine's Git sync, when its header offers it.
export function gitFix(sync: Sync, project: string, machine: MachineDetail) {
  return sync.repairButton && !machine.paused && machine.error && gitSyncWarning(machine) ? sync.repairButton(project, machine.machine_id, machine.error) : null;
}
export function SyncMachine({ sync, project, machine, extra = null }: { sync: Sync; project: string; machine: MachineDetail; extra?: ReactNode }) {
  const chip = stateChip(machine), fix = gitFix(sync, project, machine), pct = machine.paused ? null : syncPct(machine), clock = clockWarning(machine.machine_id, null), note = stateNote(machine);
  return <div className="fs-row pm-sync">
    <div className="fs-h"><b>{machine.git_only ? "Git sync" : "File sync"}</b><Chip kind={chip.wait ? "wait" : ""} text={chip.text} title={machine.error || ""} />{fix && <AgentButton item={fix} />}{extra}</div>
    {pct !== null && <div className="fs-bar"><i style={{ width: `${pct}%` }} /></div>}{!machine.git_only && <div className="fs-m">{machineMeta(machine).map((part, i) => <span key={i}>{part}</span>)}</div>}
    {clock && <div className="fs-m"><span className="clockoff" title={clock.title}>{clock.text}</span></div>}{note && <div className={machine.status === "syncthing" ? "fs-m" : "cc-err"} title={machine.error || ""}>{note}</div>}
    {machine.can_start_over && !machine.paused && <div className="fs-m"><Button small title="Syncs the folder now at this path from nothing; nothing is deleted on other machines" onClick={() => void sync.startOver(project, machine)}>Start over with this folder</Button></div>}{machine.hint && <div className="fs-m">Rescanned instead of watched. To watch it, run {machine.hint}</div>}
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
      items.push({ label: "Keep this copy", warn: true, on: () => confirmSheet({ title: "Keep this copy?", text: `It takes the place of ${conflict.original} on every machine.`, action: "Keep this copy", run: () => sync.resolve(project, conflict, true) }) },
        { label: "Keep the original", warn: true, on: () => confirmSheet({ title: "Keep the original?", text: `${conflict.original} stays and this copy is deleted on every machine.`, action: "Keep the original", run: () => sync.resolve(project, conflict, false) }) });
    }
    if (items.length) openMenu(event, conflict.path, items);
  };
  return <div className={`fs-conflict${comparing ? " open" : ""}`} onContextMenu={event => menu(event.nativeEvent)}><span className="t"><code>{conflict.path}</code><span>{conflictSub(null, conflict, machines)}</span></span><span className="acts">
    {!gitConflict(conflict.path) && <Button small title={`Shows what differs between ${conflict.original} and this copy`} onClick={() => void sync.compare(project, conflict, machines)}>{comparing ? "Hide" : "Compare"}</Button>}
    {sync.fixButton && conflict.holder && <AgentButton item={sync.fixButton(project, conflict)} />}
    {conflict.resolver ? <><Button small disabled={disabled} title={`Puts this copy in place of ${conflict.original} on every machine`} onClick={() => void sync.resolve(project, conflict, true)}>{busy === conflict.path ? "Keeping…" : "Keep this copy"}</Button><Button small disabled={disabled} title={`Keeps ${conflict.original} and deletes this copy on every machine`} onClick={() => void sync.resolve(project, conflict, false)}>Keep the original</Button></> : <span className="dmuted">Update beep-node on a machine that has it to settle it here</span>}</span>
    {comparing && <div className="fs-cmp"><div className="fs-cmph"><span>Kept <b>{conflict.original}</b></span><span>Copy <b>{conflict.path}</b></span><small>{where ? `on ${where}` : ""}</small></div>{shown === null ? <div className="dmuted fs-cmpn">Reading…</div> : typeof shown === "string" ? <div className="dmuted fs-cmpn">{shown}</div> : shown && <DiffHost node={shown} />}</div>}
  </div>;
}
export function SyncProject({ sync, project, model, machines }: { sync: Sync; project: string; model: ReturnType<Sync["page"]>; machines: ReactNode }) {
  const { detail, error, busy } = model;
  if (!detail) return <>{error && <div className="cc-err">{error}</div>}{machines}<EmptyPanel text="Loading…" /></>;
  const conflicts = conflictList(detail), waiting = waitingList(detail);
  return <>{error && <div className="cc-err">{error}</div>}<SectionHead title="Machines"><span className="fs-tools"><small>{syncSummary(detail)}</small><Button look={detail.paused ? "primary" : "secondary"} disabled={!!busy || detail.pinned || !detail.machines.length} hidden={!detail.paused && detail.machines.every(syncedBySyncthing)} title={detail.paused ? "Sync again on every machine" : "Hold sync on every machine; files stay as they are"} onClick={() => void sync.pause(project, !detail.paused)}>{busy === "pause" ? (detail.paused ? "Resuming…" : "Pausing…") : detail.paused ? "Resume" : "Pause"}</Button></span></SectionHead>
    {machines}
    {!!detail.ignored_folders?.length && <div className="fs-m">File sync for ignored folders: {detail.ignored_folders.join(", ")}</div>}
    {!!conflicts.length && <><SectionHead title="Conflicts"><small>edited on two machines at once</small></SectionHead><div className="cc-panel fs-list">{conflicts.map(conflict => <ConflictRow key={conflict.path} sync={sync} project={project} conflict={conflict} machines={detail.machines} busy={busy} paused={detail.paused} />)}</div></>}
    {!!waiting.length && <><SectionHead title="Waiting" /><div className="cc-panel fs-list">{waiting.map(item => <div className="fs-wait" key={item.path}><code>{item.path}</code><span className="dmuted">{item.machines.join(", ")}</span></div>)}</div></>}
    <div className="dmuted fs-note">{detail.machines.some(machine => machine.git_only) ? detail.ignored_folders?.length ? "Git syncs commits. Selected ignored folders use file sync. Other uncommitted edits stay local." : "Git checkouts sync commits. Uncommitted edits stay on their machine; use Bring work here under Machines for an explicit handoff." : "Builds, ignored files and links stay on each machine."}</div>
  </>;
}
