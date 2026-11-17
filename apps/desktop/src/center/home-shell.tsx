import type { ReactNode } from "react";
import { plural } from "../ui/format.ts";
import { Icon } from "../ui/icons.tsx";
import { machineDisk, machineDot, type MachineCard, type MacRow } from "../machines/machine-state.ts";
import { Button } from "../ui/kit.tsx";
import { HazeArt } from "../ui/painted-art.tsx";
import { useBuildCapabilities } from "../ui/tauri.ts";

export type HomeTab = "overview" | "git" | "machines";

interface HomeShellProps {
  machines: MachineCard[] | null;
  mac: MacRow | null;
  jump(id: string): void;
  menu(machine: MachineCard, event: MouseEvent): void;
  addMachine(): void;
  useMac(): void;
  error: string | null;
  unavailable: boolean;
  offline: string[];
  content: ReactNode;
  tabs: { key: HomeTab; label: string; count: number | null }[];
  selected: HomeTab;
  open(tab: HomeTab): void;
}

export function HomeShell({ machines, mac, jump, menu, addMachine, useMac, error, unavailable, offline, content, tabs, selected, open }: HomeShellProps) {
  const capabilities = useBuildCapabilities();
  return <>
    <HazeArt />
    {machines && <div className="cc-mstrip">{machines.map(machine => {
      const disk = machineDisk(machine).text;
      const now = machine.online ? (machine.can_host === false ? "controller" : machine.agents ? plural(machine.agents, "agent", "agents") : "idle") : machine.connecting ? "connecting" : "offline";
      const dot = machineDot(machine);
      return <button key={machine.id} type="button" className={`m${machine.online ? "" : " off"}`} onClick={() => jump(`m-${machine.id}`)} onContextMenu={event => menu(machine, event.nativeEvent)}><span className={dot.className} title={dot.title} /><b>{machine.name}</b><span>{disk === "–" || machine.connecting ? now : `${now} · ${disk}`}</span></button>;
    })}{capabilities.local_host && mac && <div className="m off" title={`This computer: ${mac.title}`}><span className="ddot off" /><b>{mac.name}</b><span className={mac.bad ? "bad" : ""}>{mac.state}</span>{mac.action && <Button look="quiet" small onClick={event => { event.stopPropagation(); useMac(); }}>{mac.action}</Button>}</div>}<Button look="quiet" small onClick={addMachine}><Icon name="plus" /> Add a machine</Button></div>}
    <div className="cc-tabs" role="tablist">{tabs.map(tab =>
      <button key={tab.key} type="button" role="tab" aria-selected={tab.key === selected} onClick={() => open(tab.key)}>
        {tab.label}{!!tab.count && <span className="n">{tab.count}</span>}
      </button>)}</div>
    {error && <div className="cc-note"><span className="ddot bad" /><span className="cc-err">Could not refresh: {error}</span></div>}
    {unavailable ? <div className="cc-note"><span className="ddot off" /><span>Machines and running sessions could not be read.</span></div> : !!offline.length && <div className="cc-note"><span className="ddot bad" /><span><b>{offline.join(", ")} {offline.length === 1 ? "is" : "are"} offline.</b> Their sessions left Running now. They come back when beep-node runs there again.</span></div>}
    {content}
  </>;
}
