import { Fragment } from "react";
import { formatAgo } from "../ui/format.ts";
import { Icon } from "../ui/icons.tsx";
import { ALL, byMachine, terminalNote, terminalWhere, type TerminalRow } from "../terminals/terminals.ts";
import { allTerms, openTerminals, termMachines } from "../terminals/view.ts";
import { openAddMachine, openProjects } from "../center/sidebar.ts";
import { state } from "../center/state.ts";
import { fsync } from "../project/sections.ts";
import { openTile } from "../project/split.ts";
import { machineCaps, machineDisk, machineDot, machineMemory, machineNow, machineOs, machineProjects, thisMacRow, type MachineCard, type MachineProject } from "./machine-state.ts";
import { showMachineMenu, useThisMac } from "./table.ts";
import { trafficRows, trafficSummary } from "./traffic-state.ts";

function Dot({ machine }: { machine: MachineCard }) {
  const { className, title } = machineDot(machine);
  return <span className={className} title={title} />;
}
function AddMachine({ className }: { className: string }) { return <button type="button" className={`dbtn ${className}`} onClick={openAddMachine}><Icon name="plus" /> Add a machine</button>; }
function Terminal({ terminal, online }: { terminal: TerminalRow; online: boolean }) {
  return <button type="button" className="cc-row tl-row" title={`${terminal.machine_name}: ${terminal.command} in ${terminal.cwd}`} onClick={() => openTerminals({ id: terminal.id })}><span className={`ddot${terminal.state === "running" && online ? "" : " off"}`} /><span className="t"><b>{terminal.name}</b><span>{terminalWhere(terminal)}</span></span><span className="r">{terminalNote(terminal, ms => formatAgo(ms, "short"))}</span></button>;
}
function Project({ project }: { project: MachineProject }) {
  return <button type="button" className="cc-row tl-row" title={project.title} onClick={() => { openProjects(project.project); openTile("sync"); }}><span className={`ddot${project.off ? " off" : ""}`} /><span className="t"><b>{project.project}</b><span>{project.meta.join(" · ")}</span></span><span className="r">{project.pct == null ? <span className={`chip${project.chip.wait ? " wait" : ""}`}>{project.chip.text}</span> : <span className="fs-pct"><span className="fs-bar"><i style={{ width: `${project.pct}%` }} /></span>{project.pct}%</span>}</span></button>;
}
function Machine({ machine, terminals }: { machine: MachineCard; terminals: TerminalRow[] }) {
  const off = machine.online ? "" : " off", caps = machine.can_host === false ? ["Controller"] : machineCaps(machine);
  const disk = machineDisk(machine), memory = machineMemory(machine), moved = trafficSummary(trafficRows(state.traffic.find(row => row.id === machine.id)));
  const menu = (event: React.MouseEvent) => showMachineMenu(machine, () => [], event.nativeEvent);
  return <><span id={`m-${machine.id}`} title={machine.name} className={`nm${off}`} onContextMenu={menu}><b><Dot machine={machine} /><span>{machine.name}</span></b><span className="os">{machine.can_host === false ? "Phone" : machineOs(machine)}</span></span><span className={`num${off}`} onContextMenu={menu}>{machine.online && machine.agents ? machine.agents : "–"}</span><span className={`num${off}${disk.low ? " low" : ""}${disk.other ? " two" : ""}`} title={disk.title || undefined} onContextMenu={menu}>{disk.text}{disk.other && <small>{disk.other}</small>}</span><span className={`num${off}`} title={memory.title || undefined} onContextMenu={menu}>{memory.text}</span><span className={`num${off}`} title={moved?.each} onContextMenu={menu}>{moved?.both ?? "–"}</span><span className={`caps${off}`} title={caps.length ? [...caps, ...machine.gpus.map(gpu => gpu.model)].join(", ") : undefined} onContextMenu={menu}>{caps.join(" · ") || "–"}</span><span className={`num${off}`} title={machine.online || machine.connecting ? undefined : `Seen ${formatAgo(machine.seen_at, "since")}`} onContextMenu={menu}>{machineNow(machine)}</span><span className="act" onContextMenu={menu}>{machine.can_host !== false && <button type="button" className="dbtn small" disabled={!machine.online} onClick={() => openTerminals({ newOn: machine.id })}>New terminal</button>}</span>{terminals.map(terminal => <Terminal key={terminal.id} terminal={terminal} online={machine.online} />)}{machineProjects(machine.id, state.sync, fsync.detail).map(project => <Project key={project.project} project={project} />)}</>;
}
export function HomeMachines({ machines }: { machines: MachineCard[] }) {
  const unlisted = thisMacRow(state.mac, machines);
  if (!machines.length && !unlisted) return <div id="sec-machines"><div className="cc-sec"><h4>Machines</h4><AddMachine className="small" /></div><div className="cc-panel cc-empty"><b>No machines linked yet</b><AddMachine className="pri" /></div></div>;
  const online = machines.filter(machine => machine.online).length, count = machines.length ? `${online} of ${machines.length} online` : "";
  const terms = byMachine(termMachines(), allTerms.list(ALL) || []);
  return <><div id="sec-machines"><div className="cc-sec"><h4>Machines{!!count && <span className="n">{count}</span>}</h4><AddMachine className="small" /></div><div className="cc-mt">{["Machine", "Agents", "Disk", "Memory", "Sent / received", "Can run", "Now", ""].map((header, index) => <span className="th" key={index}>{header}</span>)}{machines.map(machine => <Fragment key={machine.id}><Machine machine={machine} terminals={terms.find(row => row.id === machine.id)?.terms || []} /></Fragment>)}{unlisted && <><span className="nm off" id="m-this-mac"><b><span className="ddot off" /><span>{unlisted.name}</span></b><span className="os">{unlisted.sub}</span></span><span className={`caps mac-state${unlisted.bad ? " bad" : ""}`} title={unlisted.title}>{unlisted.state}</span><span className="num">{unlisted.action ? <button type="button" className="dbtn pri small" onClick={() => { void useThisMac(); }}>{unlisted.action}</button> : "–"}</span><span /></>}</div></div></>;
}

