// The Terminals page: every machine with its terminals at the left, the open terminal or the
// New terminal form at the right. makeTerminals (terminal-state.ts) holds what it shows.
import { type FormEvent, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { TerminalOutput, TerminalPage, TerminalScreen } from "./page-view.tsx";
import { NewTerminalFields } from "./new-terminal-view.tsx";
import { TerminalFooter, TerminalHeader } from "../../parts/terminals/terminal-controls.tsx";
import { confirmSheet, openMenu, type MenuItem } from "../../ui/menu.tsx";
import { Button } from "../../ui/kit.tsx";
import { ALL, byMachine, stateLabel } from "../../stores/terminals/terminal-list.ts";
import { linkLabel, viewLink, type TerminalMachine, type TerminalPlace, type TerminalRow, type Terminals } from "../../stores/terminals/terminal-state.ts";

/// The Terminals page: every machine with its terminals at the left, the
/// open terminal or the New terminal form at the right.
export function machinesPanel(terms: Terminals, machines: readonly TerminalMachine[]) {
  const { of, load, select, newOn, enter, ago, watch } = terms.page;
  enter();
  const t = of(ALL);
  if (t.list === undefined) load(ALL);

  const groups = byMachine(machines, t.list || []);
  const places = machines.filter(m => m.online).map(m => ({ id: m.id, name: m.name, folder: "" }));
  const open = t.open && (t.list || []).find(x => x.id === t.open);
  let right: ReactNode;

  if (t.form) {
    watch(null);
    right = formView(terms, places);
  } else if (open) {
    right = openView(terms, ALL, open);
  } else {
    watch(null);
    const any = (t.list || []).length > 0;
    right = <div className="cc-conn"><b>{any ? "Pick a terminal" : t.list ? "No terminals yet" : "Loading…"}</b>
      <Button look="primary" onClick={() => newOn(places[0]?.id || null)}>New terminal</Button></div>;
  }
  return <>
    {t.error && <div className="cc-err">{t.error}</div>}
    <div className="tl"><TerminalPage groups={groups} selected={t.open} ago={ago}
      open={(id: string) => select(ALL, id)} create={(id: string) => newOn(id)}>{right}</TerminalPage></div>
  </>;
}

/// The New terminal form of the Terminals page: the folder may stay empty,
/// for the machine's workspace or home folder.
function formView(terms: Terminals, places: TerminalPlace[]) {
  const { of, start, redraw, form } = terms.page;
  const t = of(ALL);
  const f = t.form!;
  const submit = () => { if (!t.busy && f.machine && places.length) void start(ALL, f); };
  return <form key={form()} className="cc-panel tm-form" onSubmit={(event: FormEvent) => { event.preventDefault(); submit(); }}>
    <NewTerminalFields
      places={places} machine={f.machine} cwd={f.cwd} command={f.command} name={f.name} busy={t.busy}
      choose={(machine: string) => { f.machine = machine; redraw(); }}
      change={(field: "cwd" | "command" | "name", value: string) => { f[field] = value; }}
      start={submit} cancel={() => { t.form = null; redraw(); }} />
  </form>;
}

/// The open terminal: its live screen, or the last lines of a view-only one.
function openView(terms: Terminals, name: string, open: TerminalRow) {
  const { of, liveOf, attach, detach, close, watch, focusOpen, redraw, ago } = terms.page;
  const t = of(name);
  const live = liveOf(name, open.id);
  const link = viewLink(live.link);
  const items: MenuItem[] = [];
  const toggleTyping = () => {
    live.watching = !live.watching;
    live.screen.setInput(live.link.kind === "attached" && !live.watching);
    if (!live.watching) live.screen.focus();
    redraw();
  };
  const detachHere = () => { detach(live); redraw(); };
  const attachHere = () => { attach(name, live); redraw(); };

  if (link === "attached" || link === "attaching") {
    items.push({ label: live.watching ? "Take control" : "View only", on: toggleTyping }, { label: "Detach", on: detachHere });
  } else if (link === "detached" || (link === "ended" && open.state === "offline")) {
    items.push({ label: "Attach", on: attachHere });
  }

  const running = open.state === "running";
  const stop = () => {
    if (!running) return void close(name, open.id);
    confirmSheet({
      title: `Stop ${open.name}?`,
      text: `It stops running on ${open.machine_name}.`,
      action: "Stop",
      run: () => close(name, open.id),
    });
  };
  items.push({ label: running ? "Kill" : "Close", warn: true, on: stop });
  let body: ReactNode;

  if (link === "view_only") {
    const o = t.output;
    body = <TerminalOutput command={`${open.machine_name} ${open.cwd} $ ${open.command}`} earlier={!!o?.earlier} lines={o ? o.lines : []} />;
    watch(name);
  } else {
    // A redraw moves the screen to the new page, which drops the focus:
    // the keys typed next would reach the window instead.
    const keep = live.screen.focused();
    body = <TerminalScreen element={live.screen.el} />;
    watch(null);
    requestAnimationFrame(() => live.screen.fit());
    if (keep) queueMicrotask(() => live.screen.focus());
  }

  return <div className="cc-panel tm-panel" key={open.id}>
    <div className="tm-head" onContextMenu={(event: ReactMouseEvent) => { event.preventDefault(); openMenu(event.nativeEvent, open.name, items); }}>
      <TerminalHeader terminal={open} state={stateLabel(open)} link={link} watching={live.watching}
        focus={() => { focusOpen(); }} toggle={toggleTyping} detach={detachHere} attach={attachHere} stop={stop} />
    </div>
    {body}
    <div className="tm-foot"><TerminalFooter active={link === "attached" && open.state === "running"} label={linkLabel(link, live.watching, open.machine_name, "tries" in live.link ? live.link.tries : 0)} started={ago(open.started_at)} /></div>
  </div>;
}
