import { useLayoutEffect, useRef } from "react";
import { patchChildren } from "../ui/patch.ts";
import { plural } from "../ui/format.ts";
import { IC } from "../ui/icons.ts";
import { machineDisk, type MachineCard, type MacRow } from "../machines/machine-state.ts";

export type HomeTab = "overview" | "git" | "machines";

export interface HomeShellProps {
  machines: MachineCard[] | null;
  mac: MacRow | null;
  jump(id: string): void;
  menu(machine: MachineCard, event: MouseEvent): void;
  addMachine(): void;
  useMac(): void;
  notices: HTMLElement[];
  content: HTMLElement[];
  tabs: { key: HomeTab; label: string; count: number | null }[];
  selected: HomeTab;
  open(tab: HomeTab): void;
}

// The remaining sections migrate independently while the shell owns navigation.
function ExistingSection({ nodes }: { nodes: HTMLElement[] }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (host.current) patchChildren(host.current, nodes);
  });
  return <div ref={host} className="home-sections" style={{ display: "contents" }} />;
}

export function HomeShell({ machines, mac, jump, menu, addMachine, useMac, notices, content, tabs, selected, open }: HomeShellProps) {
  return <>
    {machines && <div className="cc-mstrip">{machines.map(machine => {
      const disk = machineDisk(machine).text;
      const now = machine.online ? (machine.can_host === false ? "controller" : machine.agents ? plural(machine.agents, "agent", "agents") : "idle") : "offline";
      const dot = !machine.online ? " off" : machine.agents ? " busy" : "";
      const title = !machine.online ? "Offline" : machine.agents ? `Online · ${plural(machine.agents, "agent", "agents")} running` : "Online";
      return <button key={machine.id} type="button" className={`m${machine.online ? "" : " off"}`} onClick={() => jump(`m-${machine.id}`)} onContextMenu={event => menu(machine, event.nativeEvent)}><span className={`ddot${dot}`} title={title} /><b>{machine.name}</b><span>{disk === "–" ? now : `${now} · ${disk}`}</span></button>;
    })}{mac && <div className="m off" title={`This computer: ${mac.title}`}><span className="ddot off" /><b>{mac.name}</b><span className={mac.bad ? "bad" : ""}>{mac.state}</span>{mac.action && <button type="button" className="dbtn quiet small" onClick={event => { event.stopPropagation(); useMac(); }}>{mac.action}</button>}</div>}<button type="button" className="dbtn quiet small" onClick={addMachine} dangerouslySetInnerHTML={{ __html: `${IC.plus} Add a machine` }} /></div>}
    <div className="cc-tabs" role="tablist">{tabs.map(tab =>
      <button key={tab.key} type="button" role="tab" aria-selected={tab.key === selected} onClick={() => open(tab.key)}>
        {tab.label}{!!tab.count && <span className="n">{tab.count}</span>}
      </button>)}</div>
    <ExistingSection nodes={notices} />
    <ExistingSection nodes={content} />
  </>;
}
