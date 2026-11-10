import type { Ago } from "../ui/format.ts";
import { terminalNote, terminalWhere, type MachineTerminals, type TerminalRow } from "./terminals.ts";

export function TerminalMachines({ groups, selected, ago, open, create }: { groups: readonly MachineTerminals<TerminalRow>[]; selected: string | null; ago: Ago; open: (id: string) => void; create: (machine: string) => void }) {
  return <>{groups.map(machine => <div className="tl-m" key={machine.id}><h5><span className={`ddot${machine.online ? "" : " off"}`} />{machine.online ? machine.name : `${machine.name} · offline`}<button type="button" className="dbtn" disabled={!machine.online} onClick={() => create(machine.id)}>New</button></h5>
    {machine.terms.length ? machine.terms.map(terminal => <button type="button" className="tl-it" key={terminal.id} title={`${terminal.machine_name}: ${terminal.command} in ${terminal.cwd}`} aria-current={selected === terminal.id ? "true" : undefined} onClick={() => open(terminal.id)}><span className={`ddot${terminal.state === "running" ? "" : " off"}`} /><b>{terminal.name}</b><small>{terminalWhere(terminal)} · {terminalNote(terminal, ago)}</small></button>) : <div className="tl-empty">No terminals.</div>}
    {!machine.online && machine.terms.length > 0 && <div className="tl-empty">They keep running there and show again when it is back.</div>}</div>)}</>;
}
