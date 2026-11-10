import { createElement } from "react";
import { setSessionPortal } from "../agents/session-frame.tsx";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.ts";
import { closeTab, type MediaTab } from "./media.ts";
import { readError } from "./read.ts";
import { attach, detachTerm, killTerm, termStatus, type PaneTerm } from "./terminal.ts";

export function drawTermFooter(tab: MediaTab, note: string | null = null) {
  const host = tab.el, term = tab.term;
  if (!host || !term) return;
  setSessionPortal(host, createElement(TermFooter, { tab, term, note }));
}

function TermFooter({ tab, term, note }: { tab: MediaTab; term: PaneTerm; note: string | null }) {
  const reattach = () => { term.tries = 0; attach(term); };
  const kill = () => killTerm(term).then(() => { tab.term = undefined; closeTab(tab.id); });
  const close = () => {
    if (term.row.state !== "running" || term.link === "ended") {
      void kill().catch(error => drawTermFooter(tab, readError(error)));
      return;
    }
    confirmSheet({ title: `Stop ${tab.name}?`, text: `It stops running on ${term.row.machine_name}.`, action: "Stop", run: kill });
  };
  const menu = (event: React.MouseEvent) => {
    const items: MenuItem[] = [];
    if (term.link === "detached") items.push({ label: "Attach", on: reattach });
    if (term.link === "attached" || term.link === "attaching") items.push({ label: "Detach", on: () => { detachTerm(term); drawTermFooter(tab); } });
    items.push({ label: term.row.state === "running" && term.link !== "ended" ? "Kill…" : "Close", warn: true, on: close });
    openMenu(event.nativeEvent, tab.name, items);
  };
  return <div className="agpacts" onContextMenu={menu}><span className="meta mono">{note ?? termStatus(term, term.row.machine_name)}</span><span className="sp" /><button className="dbtn" data-tattach="1" hidden={term.link !== "detached"} onClick={reattach}>Attach</button><button className="dbtn warn" data-tkill="1" onClick={close}>Close</button></div>;
}
