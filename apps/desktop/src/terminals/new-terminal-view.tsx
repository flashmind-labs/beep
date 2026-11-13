import { ChipToggle } from "../ui/kit.tsx";
import type { TerminalPlace } from "./terminals.ts";
import { Button } from "../ui/kit.tsx";

export function NewTerminalFields({ places, machine, cwd, command, name, busy, choose, change, start, cancel }: {
  places: readonly TerminalPlace[]; machine: string | null; cwd: string; command: string; name: string; busy: boolean;
  choose: (id: string) => void; change: (field: "cwd" | "command" | "name", value: string) => void; start: () => void; cancel: () => void;
}) {
  return <><b className="tm-title">New terminal</b><div className="tm-f"><span>Machine</span>{places.length ? <div className="tm-chips">{places.map(place => <ChipToggle key={place.id} label={place.name} selected={place.id === machine} onToggle={() => choose(place.id)} />)}</div> : <span className="dmuted">No machine is online</span>}</div>
    <label className="tm-f"><span>Folder</span><input aria-label="Folder" defaultValue={cwd} placeholder="Empty for the workspace or home folder" spellCheck={false} onChange={event => change("cwd", event.currentTarget.value)} /></label>
    <label className="tm-f"><span>Command</span><input aria-label="Command" defaultValue={command} placeholder="Empty for your shell" spellCheck={false} onChange={event => change("command", event.currentTarget.value)} /></label>
    <label className="tm-f"><span>Name</span><input aria-label="Name" defaultValue={name} placeholder="The command, or shell, when empty" spellCheck={false} onChange={event => change("name", event.currentTarget.value)} /></label>
    <div className="in-acts"><Button look="primary" disabled={busy || !places.length} onClick={start}>{busy ? "Starting…" : "Open"}</Button><Button look="quiet" onClick={cancel}>Cancel</Button></div></>;
}
