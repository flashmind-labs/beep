import { useLayoutEffect, useRef, type ReactNode } from "react";
import { TerminalMachines } from "./machines-view.tsx";
import type { Ago } from "../ui/dom.ts";
import type { MachineTerminals, TerminalRow } from "./terminals.ts";

export function TerminalPage({ groups, selected, ago, open, create, children }: {
  groups: readonly MachineTerminals<TerminalRow>[]; selected: string | null; ago: Ago;
  open(id: string): void; create(machine: string): void; children: ReactNode;
}) {
  return <><div className="tl-list"><TerminalMachines {...{ groups, selected, ago, open, create }} /></div>{children}</>;
}

export function TerminalScreen({ element }: { element: HTMLElement }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const container = host.current;
    if (!container) return;
    container.append(element);
    return () => { element.remove(); };
  }, [element]);
  return <div ref={host} style={{ display: "contents" }} />;
}
