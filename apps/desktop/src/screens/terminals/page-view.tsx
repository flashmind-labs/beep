import { useLayoutEffect, useRef, type ReactNode } from "react";
import { TerminalMachines } from "./machines-view.tsx";
import type { Ago } from "../../ui/format.ts";
import { terminalOutput, type MachineTerminals, type TerminalRow } from "../../stores/terminals/terminal-state.ts";

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

/// A view-only terminal's last lines. The element stays mounted across reads,
/// so focus, selection and an upward scroll survive; it follows new lines only
/// while scrolled to the end.
export function TerminalOutput({ command, earlier, lines }: { command: string; earlier: boolean; lines: readonly string[] }) {
  const screen = terminalOutput, follow = useRef(true);
  const text = lines.join("\n");

  useLayoutEffect(() => {
    const element = screen.current;
    if (element && follow.current) element.scrollTop = element.scrollHeight;
  }, [text, earlier]);

  const scrolled = () => {
    const element = screen.current;
    if (element) follow.current = element.scrollHeight - element.scrollTop - element.clientHeight < 4;
  };

  return <pre ref={screen} className="tm-screen" tabIndex={0} aria-label="Terminal output" onScroll={scrolled}><span className="tm-cmd">{command}</span>{"\n"}{earlier ? "…\n" : ""}{text}</pre>;
}
