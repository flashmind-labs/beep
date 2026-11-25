import { create } from "zustand";
import { formatAgo, plural } from "../ui/format.ts";
import { Icon } from "../ui/icons.tsx";
import type { TerminalRow } from "../terminals/terminals.tsx";
import { ALL, byMachine, terminalNote, terminalWhere } from "../terminals/terminal-list.ts";
import { allTerms, openTerminals, termMachines } from "../center/allterms.ts";
import { openAddMachine, openProjects } from "../project/addflow.ts";
import { projectWindowProps } from "../project/projectops.ts";
import { state } from "../center/state.ts";
import { MACHINES } from "./machine-store.ts";
import { fsync } from "../project/sections.ts";
import { openTile } from "../project/tiles.tsx";
import { machineCaps, machineDisk, machineDot, machineMemory, machineNow, machineOs, machineProjects, projectsSummary, thisMacRow, type MacRow, type MachineCard, type MachineProject } from "./machine-state.ts";
import { showMachineMenu, useThisMac } from "./table.ts";
import { trafficRows, trafficSummary } from "./traffic-state.ts";
import { Button, Chip, SectionHead, type ButtonLook } from "../ui/kit.tsx";
import { useBuildCapabilities } from "../ui/tauri.ts";

/// The machines whose project list is open, for as long as the window lives.
const useOpenFolds = create<{ open: ReadonlySet<string>; toggle(id: string): void }>(set => ({
  open: new Set(),
  toggle: id => set(({ open }) => ({ open: new Set(open.has(id) ? [...open].filter(x => x !== id) : [...open, id]) })),
}));

function Dot({ machine }: { machine: MachineCard }) {
  const { className, title } = machineDot(machine);
  return <span className={className} title={title} />;
}
function AddMachine({ look = "secondary", small = false }: { look?: ButtonLook; small?: boolean }) { return <Button look={look} small={small} onClick={openAddMachine}><Icon name="plus" /> Add a machine</Button>; }
function Terminal({ terminal, online }: { terminal: TerminalRow; online: boolean }) {
  return <button type="button" className="cc-row tl-row" title={`${terminal.machine_name}: ${terminal.command} in ${terminal.cwd}`} onClick={() => openTerminals({ id: terminal.id })}><span className={`ddot${terminal.state === "running" && online ? "" : " off"}`} /><span className="t"><b>{terminal.name}</b><span>{terminalWhere(terminal)}</span></span><span className="r">{terminalNote(terminal, ms => formatAgo(ms, "short"))}</span></button>;
}
function Project({ project, compact = false }: { project: MachineProject; compact?: boolean }) {
  const props = projectWindowProps(project.project);
  return <Button look="plain" className="cc-row tl-row" title={project.title} {...props.clicks} {...props.menu} onClick={() => { openProjects(project.project); openTile("sync"); }}><span className={`ddot${project.off ? " off" : ""}`} /><span className="t">{compact ? project.project : <><b>{project.project}</b><span>{project.meta.join(" · ")}</span></>}</span>{!compact && <span className="r">{project.pct == null ? <span className={`chip${project.chip.wait ? " wait" : ""}`}>{project.chip.text}</span> : <span className="fs-pct"><span className="fs-bar"><i style={{ width: `${project.pct}%` }} /></span>{project.pct}%</span>}</span>}</Button>;
}
function Stat({ label, title, bar, low = false, children }: { label: string; title?: string; bar?: number | null; low?: boolean; children: React.ReactNode }) {
  return <div className="mstat" title={title || undefined}><span className="ml">{label}</span>{children}{bar != null && <div className={`mbar${low ? " low" : ""}`}><i style={{ width: `${Math.round(100 * bar)}%` }} /></div>}</div>;
}
function Folded({ machine, projects }: { machine: MachineCard; projects: MachineProject[] }) {
  const open = useOpenFolds(fold => fold.open.has(machine.id)), toggle = useOpenFolds(fold => fold.toggle);
  return <div className="mlist"><button type="button" className="cc-row tl-row mfold" aria-expanded={open} onClick={() => toggle(machine.id)}><Icon name="chev" /><span className="t"><b>{plural(projects.length, "project", "projects")} here</b><span>· {projectsSummary(projects)}</span></span></button>{open && <div className="mgrid">{projects.map(project => <Project key={project.project} project={project} compact />)}</div>}</div>;
}
function Machine({ machine, terminals }: { machine: MachineCard; terminals: TerminalRow[] }) {
  const caps = machine.can_host === false ? ["Controller"] : machineCaps(machine);
  const disk = machineDisk(machine), memory = machineMemory(machine), moved = trafficSummary(trafficRows(MACHINES.traffic.find(row => row.id === machine.id)));
  const projects = machine.online ? machineProjects(machine.id, state.sync, fsync.detail) : [], live = projects.filter(project => project.pct != null || project.chip.wait);
  const menu = (event: React.MouseEvent) => showMachineMenu(machine, () => [], event.nativeEvent);
  return <div id={`m-${machine.id}`} className={`mcard${machine.online ? "" : " off"}`} onContextMenu={menu}>
    <div className="mtop"><span className="mname" title={machine.name}><Dot machine={machine} /><b>{machine.name}</b><span className="mnote">{[machine.can_host === false ? "Controller" : machineOs(machine), machineNow(machine)].filter(Boolean).join(" · ")}</span></span><span className="act">{machine.can_host !== false && <Button small disabled={!machine.online} onClick={() => openTerminals({ newOn: machine.id })}>New terminal</Button>}</span></div>
    {machine.online && <>
      <div className="mstats">
        <Stat label="Disk" title={disk.title} bar={disk.used} low={disk.low}><span className="v">{disk.text}</span>{disk.other && <small>{disk.other}</small>}</Stat>
        <Stat label="Memory" title={memory.title} bar={memory.used}><span className="v">{memory.text}</span></Stat>
        <Stat label="Sent / received" title={moved?.each}><span className="v">{moved?.both ?? "–"}</span></Stat>
        <Stat label="Can run" title={caps.length ? [...caps, ...machine.gpus.map(gpu => gpu.model)].join(", ") : undefined}>{caps.length ? <span className="mcaps">{caps.map(name => <Chip key={name} kind="" text={name} />)}</span> : <span className="v">–</span>}</Stat>
      </div>
      {(terminals.length > 0 || live.length > 0) && <div className="mlist">{terminals.map(terminal => <Terminal key={terminal.id} terminal={terminal} online={machine.online} />)}{live.map(project => <Project key={project.project} project={project} />)}</div>}
      {projects.length > 0 && <Folded machine={machine} projects={projects} />}
    </>}
  </div>;
}
function ThisMac({ mac }: { mac: MacRow }) {
  return <div id="m-this-mac" className="mcard off"><div className="mtop"><span className="mname" title={mac.title}><span className="ddot off" /><b>{mac.name}</b><span className={`mnote${mac.bad ? " bad" : ""}`}>{[mac.sub, mac.state].filter(Boolean).join(" · ")}</span></span>{mac.action && <span className="act keep"><Button look="primary" small onClick={() => { void useThisMac(); }}>{mac.action}</Button></span>}</div></div>;
}
export function HomeMachines({ machines }: { machines: MachineCard[] }) {
  const capabilities = useBuildCapabilities();
  const unlisted = capabilities.local_host ? thisMacRow(MACHINES.mac, machines) : null;
  if (!machines.length && !unlisted) return <div id="sec-machines"><SectionHead title="Machines"><AddMachine small /></SectionHead><div className="cc-panel cc-empty"><b>No machines linked yet</b><AddMachine look="primary" /></div></div>;
  const online = machines.filter(machine => machine.online).length, count = machines.length ? `${online} of ${machines.length} online` : "";
  const terms = byMachine(termMachines(), allTerms.list(ALL) || []);
  return <div id="sec-machines"><SectionHead title="Machines" count={count}><AddMachine small /></SectionHead><div className="mcards">{machines.map(machine => <Machine key={machine.id} machine={machine} terminals={terms.find(row => row.id === machine.id)?.terms || []} />)}{unlisted && <ThisMac mac={unlisted} />}</div></div>;
}
