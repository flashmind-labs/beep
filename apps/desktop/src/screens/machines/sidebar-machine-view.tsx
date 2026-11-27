import { plural, spoken } from "../../ui/format.ts";
import { jump } from "../center/home.tsx";
import { machineDot, type MachineCard, type MacRow } from "../../parts/machines/machine-state.ts";
import { showMachineMenu, useThisMac } from "./table.ts";
import { Button } from "../../ui/kit.tsx";

export function SidebarMachine({ machine }: { machine: MachineCard | { thisMac: MacRow } }) {
  if ("thisMac" in machine) {
    const mac = machine.thisMac, title = `This computer: ${mac.title}`;
    if (mac.action) return <div className="cc-si" title={title} onClick={() => jump("m-this-mac")}><span className="ddot off" /><span className="tt">{mac.name}</span><Button look="quiet" small className="mac-add" onClick={event => { event.stopPropagation(); void useThisMac(); }}>{mac.action}</Button></div>;
    return <button type="button" className="cc-si" title={title} aria-label={spoken(mac.name, mac.state)} onClick={() => jump("m-this-mac")}><span className="ddot off" /><span className="tt">{mac.name}</span>{" "}<span className={`n mac-state${mac.bad ? " bad" : ""}`}>{mac.state}</span></button>;
  }
  const status = machine.online ? machine.agents ? `${plural(machine.agents, "agent", "agents")} running` : "online" : machine.connecting ? "connecting" : "offline";
  const dot = machineDot(machine);
  return <button type="button" className="cc-si" title={`${machine.name}: ${status}`} aria-label={spoken(machine.name, machine.agents)} onClick={() => jump(`m-${machine.id}`)} onContextMenu={event => showMachineMenu(machine, () => [], event.nativeEvent)}><span className={dot.className} title={dot.title} /><span className="tt">{machine.name}</span>{!!machine.agents && <>{" "}<span className="n">{machine.agents}</span></>}</button>;
}
