import type { TerminalRow } from "./terminals.ts";

export type TerminalControlLink = "attaching" | "attached" | "detached" | "ended" | "view_only";
export function TerminalHeader({ terminal, state, link, watching, focus, toggle, detach, attach, stop }: {
  terminal: TerminalRow; state: string; link: TerminalControlLink; watching: boolean;
  focus: () => void; toggle: () => void; detach: () => void; attach: () => void; stop: () => void;
}) {
  const following = link === "attached" || link === "attaching", canAttach = link === "detached" || (link === "ended" && terminal.state === "offline");
  return <><span><b>{terminal.name}</b> {terminal.machine_name} · {terminal.cwd}</span><span className="tm-st">{state}</span>
    <button type="button" className="dbtn small" title="Focus terminal (Shift+Esc)" onClick={focus}>Focus</button>
    {following && <><button type="button" className="dbtn small" title={watching ? "Type into the terminal again" : "Stop typing into it, keep watching"} onClick={toggle}>{watching ? "Take control" : "View only"}</button><button type="button" className="dbtn small" title="Stop following it here; it keeps running" onClick={detach}>Detach</button></>}
    {canAttach && <button type="button" className="dbtn small pri" onClick={attach}>Attach</button>}
    <button type="button" className="dbtn small warn" onClick={stop}>{terminal.state === "running" ? "Kill…" : "Close"}</button></>;
}

export function TerminalFooter({ active, label, started }: { active: boolean; label: string; started: string }) {
  return <><span><span className={`ddot${active ? "" : " off"}`} /> {label}</span><span>started {started}</span></>;
}
