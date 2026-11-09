import { Fragment } from "react";
import { ago, plural, since } from "../ui/format.ts";
import { IC } from "../ui/icons.ts";
import { ALL, byMachine, terminalNote, terminalWhere, type TerminalRow } from "../terminals/terminals.ts";
import { allTerms, openTerminals, termMachines } from "../terminals/view.ts";
import { openAddMachine, openProjects } from "../center/sidebar.ts";
import { state } from "../center/state.ts";
import { fsync } from "../project/sections.ts";
import { openTile } from "../project/split.ts";
import { copyJoinLine } from "./notjoined.ts";
import { notJoinedSub, splitJoined } from "./notjoined-state.ts";
import { machineCaps, machineDisk, machineMemory, machineNow, machineOs, machineProjects, thisMacRow, type MachineCard, type MachineProject } from "./machine-state.ts";
import { showMachineMenu, useThisMac } from "./table.ts";
import { trafficRows, trafficSummary } from "./traffic-state.ts";

function Dot({ machine }: { machine: MachineCard }) {
  const status = !machine.online ? " off" : machine.agents ? " busy" : "";
  const title = !machine.online ? "Offline" : machine.agents ? `Online · ${plural(machine.agents, "agent", "agents")} running` : "Online";
  return <span className={`ddot${status}`} title={title} />;
}
function AddMachine({ className }: { className: string }) { return <button type="button" className={`dbtn ${className}`} onClick={openAddMachine} dangerouslySetInnerHTML={{ __html: `${IC.plus} Add a machine` }} />; }
function Terminal({ terminal, online }: { terminal: TerminalRow; online: boolean }) {
  return <button type="button" className="cc-row tl-row" title={`${terminal.machine_name}: ${terminal.command} in ${terminal.cwd}`} onClick={() => openTerminals({ id: terminal.id })}><span className={`ddot${terminal.state === "running" && online ? "" : " off"}`} /><span className="t"><b>{terminal.name}</b><span>{terminalWhere(terminal)}</span></span><span className="r">{terminalNote(terminal, ago)}</span></button>;
}
function Project({ project }: { project: MachineProject }) {
  return <button type="button" className="cc-row tl-row" title={project.title} onClick={() => { openProjects(project.project); openTile("sync"); }}><span className={`ddot${project.off ? " off" : ""}`} /><span className="t"><b>{project.project}</b><span>{project.meta.join(" · ")}</span></span><span className="r">{project.pct == null ? <span className={`chip${project.chip.wait ? " wait" : ""}`}>{project.chip.text}</span> : <span className="fs-pct"><span className="fs-bar"><i style={{ width: `${project.pct}%` }} /></span>{project.pct}%</span>}</span></button>;
}
function Machine({ machine, terminals }: { machine: MachineCard; terminals: TerminalRow[] }) {
  const off = machine.online ? "" : " off", caps = machine.can_host === false ? ["Controller"] : machineCaps(machine);
  const disk = machineDisk(machine), memory = machineMemory(machine), moved = trafficSummary(trafficRows(state.traffic.find(row => row.id === machine.id)));
  const menu = (event: React.MouseEvent) => showMachineMenu(machine, () => [], event.nativeEvent);
  return <><span id={`m-${machine.id}`} title={machine.name} className={`nm${off}`} onContextMenu={menu}><b><Dot machine={machine} /><span>{machine.name}</span></b><span className="os">{machine.can_host === false ? "Phone" : machineOs(machine)}</span></span><span className={`num${off}`} onContextMenu={menu}>{machine.online && machine.agents ? machine.agents : "–"}</span><span className={`num${off}${disk.low ? " low" : ""}${disk.other ? " two" : ""}`} title={disk.title || undefined} onContextMenu={menu}>{disk.text}{disk.other && <small>{disk.other}</small>}</span><span className={`num${off}`} title={memory.title || undefined} onContextMenu={menu}>{memory.text}</span><span className={`num${off}`} title={moved?.each} onContextMenu={menu}>{moved?.both ?? "–"}</span><span className={`caps${off}`} title={caps.length ? [...caps, ...machine.gpus.map(gpu => gpu.model)].join(", ") : undefined} onContextMenu={menu}>{caps.join(" · ") || "–"}</span><span className={`num${off}`} title={machine.online ? undefined : `Seen ${since(machine.seen_at)}`} onContextMenu={menu}>{machineNow(machine)}</span><span className="act" onContextMenu={menu}>{machine.can_host !== false && <button type="button" className="dbtn small" disabled={!machine.online} onClick={() => openTerminals({ newOn: machine.id })}>New terminal</button>}</span>{terminals.map(terminal => <Terminal key={terminal.id} terminal={terminal} online={machine.online} />)}{machineProjects(machine.id, state.sync, fsync.detail).map(project => <Project key={project.project} project={project} />)}</>;
}
function NotJoined({ machines }: { machines: MachineCard[] }) {
  return <div id="sec-notjoined"><div className="cc-sec"><h4>Not in the mesh yet<span className="n">{machines.length}</span></h4></div><div className="cc-panel">{machines.map(machine => <div className="cc-row cc-line" key={machine.id}><Dot machine={machine} /><button type="button" className="t" title={machine.name}><b>{machine.name}</b><span>{notJoinedSub(machineOs(machine))}</span></button><span className="r"><span className="chip blocked">Not joined</span><button type="button" className="dbtn small" onClick={event => { event.stopPropagation(); void copyJoinLine(event.currentTarget); }}>Copy join line</button></span></div>)}</div></div>;
}
export function HomeMachines({ machines }: { machines: MachineCard[] }) {
  const unlisted = thisMacRow(state.mac, machines);
  if (!machines.length && !unlisted) return <div id="sec-machines"><div className="cc-sec"><h4>Machines</h4><AddMachine className="small" /></div><div className="cc-panel cc-empty"><b>No machines linked yet</b><AddMachine className="pri" /></div></div>;
  const { mesh, outside } = splitJoined(machines), mac = thisMacRow(state.mac, mesh), online = machines.filter(machine => machine.online).length;
  const count = outside.length ? mesh.length : machines.length ? `${online} of ${machines.length} online` : "";
  const terms = byMachine(termMachines(), allTerms.list(ALL) || []);
  return <><div id="sec-machines"><div className="cc-sec"><h4>{outside.length ? "In the mesh" : "Machines"}{!!count && <span className="n">{count}</span>}</h4><AddMachine className="small" /></div><div className="cc-mt">{["Machine", "Agents", "Disk", "Memory", "Sent / received", "Can run", "Now", ""].map((header, index) => <span className="th" key={index}>{header}</span>)}{mesh.map(machine => <Fragment key={machine.id}><Machine machine={machine} terminals={terms.find(row => row.id === machine.id)?.terms || []} /></Fragment>)}{mac && <><span className="nm off" id="m-this-mac"><b><span className="ddot off" /><span>{mac.name}</span></b><span className="os">{mac.sub}</span></span><span className={`caps mac-state${mac.bad ? " bad" : ""}`} title={mac.title}>{mac.state}</span><span className="num">{mac.action ? <button type="button" className="dbtn pri small" onClick={() => { void useThisMac(); }}>{mac.action}</button> : "–"}</span><span /></>}</div></div>{!!outside.length && <NotJoined machines={outside} />}</>;
}
