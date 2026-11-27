import type { TerminalRow } from "../../stores/terminals/terminal-state.ts";
import { Button } from "../../ui/kit.tsx";

type TerminalControlLink = "attaching" | "attached" | "detached" | "ended" | "view_only";
export function TerminalHeader({ terminal, state, link, watching, focus, toggle, detach, attach, stop }: {
  terminal: TerminalRow; state: string; link: TerminalControlLink; watching: boolean;
  focus: () => void; toggle: () => void; detach: () => void; attach: () => void; stop: () => void;
}) {
  const following = link === "attached" || link === "attaching", canAttach = link === "detached" || (link === "ended" && terminal.state === "offline");
  return <><span><b>{terminal.name}</b> {terminal.machine_name} · {terminal.cwd}</span><span className="tm-st">{state}</span>
    <Button small title="Focus terminal (Shift+Esc)" onClick={focus}>Focus</Button>
    {following && <><Button small title={watching ? "Type into the terminal again" : "Stop typing into it, keep watching"} onClick={toggle}>{watching ? "Take control" : "View only"}</Button><Button small title="Stop following it here; it keeps running" onClick={detach}>Detach</Button></>}
    {canAttach && <Button look="primary" small onClick={attach}>Attach</Button>}
    <Button danger small onClick={stop}>{terminal.state === "running" ? "Kill" : "Close"}</Button></>;
}

export function TerminalFooter({ active, label, started }: { active: boolean; label: string; started: string }) {
  return <><span><span className={`ddot${active ? "" : " off"}`} /> {label}</span><span>started {started}</span></>;
}
