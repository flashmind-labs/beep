import { useLayoutEffect, useRef, type MouseEvent } from "react";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.tsx";
import type { MediaTab } from "./pane-state.ts";
import { noteTab, stopTerminalTab, touchTab } from "./pane-tabs.ts";
import { readError } from "./read-file.ts";
import { attach, detachTerm, termStatus, type PaneTerm } from "./terminal.ts";
import { terminalFollowing } from "../terminals/terminals.tsx";
import { Button } from "../ui/kit.tsx";

/// A terminal tab: its live screen, and a footer to attach again or stop it.
/// The screen draws itself, so its element stays a native child of the view;
/// moved to the other dock, the same screen goes along.
export function TermView({ tab, term, hidden }: { tab: MediaTab; term: PaneTerm; hidden: boolean }) {
  const host = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const el = host.current;
    if (!el) return;
    el.prepend(term.screen.el);
    return () => term.screen.el.remove();
  }, [term]);

  return <div className="agpview term" hidden={hidden} ref={host}><TermFooter tab={tab} term={term} /></div>;
}

function TermFooter({ tab, term }: { tab: MediaTab; term: PaneTerm }) {
  const reattach = () => attach(term);
  const kill = () => stopTerminalTab(tab);
  const close = () => {
    if (tab.closed) return;
    if (term.row.state !== "running" || term.link.kind === "ended") {
      void kill().catch(error => noteTab(tab, readError(error)));
      return;
    }
    confirmSheet({ title: `Stop ${tab.name}?`, text: `It stops running on ${term.row.machine_name}.`, action: "Stop", run: kill });
  };
  const menu = (event: MouseEvent) => {
    const items: MenuItem[] = [];
    if (term.link.kind === "detached") items.push({ label: "Attach", on: reattach });
    if (terminalFollowing(term.link)) items.push({ label: "Detach", on: () => { detachTerm(term); touchTab(tab); } });
    if (!tab.closed) items.push({ label: term.row.state === "running" && term.link.kind !== "ended" ? "Kill" : "Close", warn: true, on: close });
    if (items.length) openMenu(event.nativeEvent, tab.name, items);
  };
  return <div className="agpacts" onContextMenu={menu}><span className="meta mono">{tab.note ?? termStatus(term, term.row.machine_name)}</span><span className="sp" /><Button data-tattach="1" hidden={term.link.kind !== "detached"} onClick={reattach}>Attach</Button><Button danger disabled={tab.closed === true} data-tkill="1" onClick={close}>Close</Button></div>;
}
