import { plural } from "../ui/format.ts";
import { jump } from "../center/home.ts";
import { machineDot, type MachineCard, type MacRow } from "./machine-state.ts";
import { showMachineMenu, useThisMac } from "./table.ts";

export function SidebarMachine({ machine }: { machine: MachineCard | { thisMac: MacRow } }) {
  if ("thisMac" in machine) {
    const mac = machine.thisMac, title = `This computer: ${mac.title}`;
    if (mac.action) return <div className="cc-si" title={title} onClick={() => jump("m-this-mac")}><span className="ddot off" /><span className="tt">{mac.name}</span><button type="button" className="dbtn quiet small mac-add" onClick={event => { event.stopPropagation(); void useThisMac(); }}>{mac.action}</button></div>;
    return <button type="button" className="cc-si" title={title} aria-label={mac.name} onClick={() => jump("m-this-mac")}><span className="ddot off" /><span className="tt">{mac.name}</span><span className={`n mac-state${mac.bad ? " bad" : ""}`}>{mac.state}</span></button>;
  }
  const status = machine.online ? machine.agents ? `${plural(machine.agents, "agent", "agents")} running` : "online" : machine.connecting ? "connecting" : "offline";
  const dot = machineDot(machine);
  return <button type="button" className="cc-si" title={`${machine.name}: ${status}`} aria-label={machine.name} onClick={() => jump(`m-${machine.id}`)} onContextMenu={event => showMachineMenu(machine, () => [], event.nativeEvent)}><span className={dot.className} title={dot.title} /><span className="tt">{machine.name}</span>{!!machine.agents && <span className="n">{machine.agents}</span>}</button>;
}
