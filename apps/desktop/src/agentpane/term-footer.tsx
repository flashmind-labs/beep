import { useLayoutEffect, useRef, type MouseEvent } from "react";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.tsx";
import { closeTab, type MediaTab } from "./media.ts";
import { noteTab, touchTab } from "./media-draw.ts";
import { readError } from "./read.ts";
import { attach, detachTerm, killTerm, termStatus, type PaneTerm } from "./terminal.ts";
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
  const reattach = () => { term.tries = 0; attach(term); };
  const kill = () => killTerm(term).then(() => { tab.term = undefined; closeTab(tab.id); });
  const close = () => {
    if (term.row.state !== "running" || term.link === "ended") {
      void kill().catch(error => noteTab(tab, readError(error)));
      return;
    }
    confirmSheet({ title: `Stop ${tab.name}?`, text: `It stops running on ${term.row.machine_name}.`, action: "Stop", run: kill });
  };
  const menu = (event: MouseEvent) => {
    const items: MenuItem[] = [];
    if (term.link === "detached") items.push({ label: "Attach", on: reattach });
    if (term.link === "attached" || term.link === "attaching") items.push({ label: "Detach", on: () => { detachTerm(term); touchTab(tab); } });
    items.push({ label: term.row.state === "running" && term.link !== "ended" ? "Kill…" : "Close", warn: true, on: close });
    openMenu(event.nativeEvent, tab.name, items);
  };
  return <div className="agpacts" onContextMenu={menu}><span className="meta mono">{tab.note ?? termStatus(term, term.row.machine_name)}</span><span className="sp" /><Button data-tattach="1" hidden={term.link !== "detached"} onClick={reattach}>Attach</Button><Button danger data-tkill="1" onClick={close}>Close</Button></div>;
}
