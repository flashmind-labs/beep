import { createElement } from "react";
import { placeWindowParts } from "./window-root.ts";
import { windowHost } from "./window-shell.tsx";
import { NoticesView } from "./notices-view.tsx";
// The bell at the right end of every command center header, beside the usage
// pill, and the notifications pane it opens: what the assistant told the user
// on its own (a mail digest, an arrival, a review request), and what the mesh
// told this Mac (a coding agent finished or asks, sync conflicts, a failed
// move, a machine that went offline). Alerts are notifications, not chats,
// until the user replies in one.
import { type PaneTarget, pane } from "../bar/pane.ts";
import type { ProjectsArg } from "./state.ts";
import { reason } from "../ui/format.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { openSettings } from "../settings/sheet.ts";
import { commandKey, WINDOWS } from "../ui/platform.ts";
import { type Alerts, type Held, type Item, type Note, bySide, find, fromAlert, fromNote, markRead, merge, opens, skillName, unreadIds, without } from "./notices-state.ts";
import { go } from "./views.ts";

/// Opens the pane, on alert `session` when one is named. Set by `mountNotices`.
let show: (session: string | null) => void = () => {};

/// A notification's click: the pane with that alert opened.
export function showNotices(session: string | null) {
  show(session);
}

/// Draws the header's bell and keeps it current.
export function mountNotices() {
  const host = windowHost("bell");
  /// The server's alerts, `null` with no server, and the mesh's notes.
  let alerts: Item[] | null = null;
  let notes: Item[] = [];
  let open = false;
  let opened: string | null = null;
  let returnFocus: HTMLElement | null = null;
  /// An alert a click asked for before the list had come.
  let wanted: string | null = null;
  /// Asks being answered, and why an answer failed, by id.
  const answering = new Set<string>();
  const failed = new Map<string, string>();
  /// Skill conflicts with a copy being kept, by id: the copy's hash and the note as it was.
  const keeping = new Map<string, { hash: string; was: string }>();
  /// What a note says, read or not.
  const told = (i: Item | null) => JSON.stringify(i && { ...i, read: true });

  const items = () => merge(alerts, notes);

  const setAlerts = (next: Alerts) => {
    const list = next && next.map(fromAlert);
    // An opened alert that did not change keeps its view, and the reply typed in it.
    const same = open && opened && JSON.stringify(find(alerts ?? [], opened)) === JSON.stringify(find(list ?? [], opened));
    alerts = list;
    if (same) return drawBell();
    if (wanted && find(alerts ?? [], wanted)) opened = wanted;
    wanted = null;
    // An opened alert that is gone, dismissed or a chat now, shows the list.
    if (opened && !find(items(), opened)) opened = null;
    if (opened) read([opened]);
    draw();
  };

  const setNotes = (next: Note[]) => {
    notes = next.map(fromNote);
    // A conflict being kept waits until the node tells it again.
    for (const [id, k] of keeping) if (told(find(notes, id)) !== k.was) keeping.delete(id);
    // The list is redrawn unless an alert is open, with a reply maybe typed in it.
    if (open && opened && find(alerts ?? [], opened)) drawBell(); else draw();
  };

  const focusList = (id?: string | null) => {
    const rows = [...host.querySelectorAll<HTMLElement>(".nt-row")];
    (rows.find(r => r.dataset.id === id) ?? rows[0] ?? host.querySelector<HTMLElement>(".nt-pop"))?.focus();
  };

  const backToList = () => {
    const id = opened;
    opened = null;
    draw();
    focusList(id);
  };

  const toggle = (to: boolean, keyboard = false) => {
    if (to && !open) returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    open = to;
    if (!to) opened = null;
    draw();
    // Opening the pane reads what it shows; the rows keep their dots until it closes.
    if (to) read(unreadIds(items()), false);

    if (keyboard) {
      if (to) focusList();
      else (returnFocus?.isConnected ? returnFocus : host.querySelector<HTMLElement>(".nt-bell"))?.focus();
    }
  };

  const drawBell = () => draw();
  const draw = () => {
    const active = document.activeElement;
    const inPop = active instanceof HTMLElement && host.contains(active) && !!active.closest(".nt-pop");
    const focused = inPop ? active.closest<HTMLElement>(".nt-row") : null;
    const index = focused ? [...host.querySelectorAll(".nt-row")].indexOf(focused) : -1;
    const action = focused ? [...focused.querySelectorAll("button")].indexOf(active as HTMLButtonElement) : -1;
    const all = items(), shown = opened ? find(all, opened) : null;
    placeWindowParts({ notices: createElement(NoticesView, {
      items: all, open, shown, answering, failed, keeping, toggle: () => toggle(!open), back: backToList,
      dismiss, openItem, answer, keep: keepCopy, reply,
    }) });
    if (focused && !focused.isConnected && open) {
      const rows = [...host.querySelectorAll<HTMLElement>(".nt-row")];
      const row = rows.find(r => r.dataset.id === focused.dataset.id) ?? rows[Math.min(index, rows.length - 1)];
      const control = row?.querySelectorAll("button")[action];
      (control && !control.disabled ? control : row ?? host.querySelector<HTMLElement>(".nt-pop button") ?? host.querySelector<HTMLElement>(".nt-pop"))?.focus();
    } else if (inPop && open && !shown && active === host.querySelector(".nt-pop")) focusList();
  };

  /// Marks `list` read, here and where it is kept; `redraw` false leaves the pane as drawn.
  const read = (list: string[], redraw = true) => {
    const fresh = list.filter(id => find(items(), id)?.read === false);
    if (!fresh.length) return;
    const { server, mesh } = bySide(items(), fresh);
    if (alerts) alerts = markRead(alerts, server);
    notes = markRead(notes, mesh);
    if (redraw) draw(); else drawBell();
    if (server.length) invoke("alerts_read", { ids: server }).catch(e => console.warn("reading alerts:", e));
    if (mesh.length) invoke("notes_read", { ids: mesh }).catch(e => console.warn("reading notifications:", e));
  };

  const dismiss = (list: string[]) => {
    if (!list.length) return;
    const { server, mesh } = bySide(items(), list);
    if (alerts) alerts = without(alerts, server);
    notes = without(notes, mesh);
    if (opened && list.includes(opened)) opened = null;
    draw();
    if (server.length) invoke("alerts_dismiss", { ids: server }).catch(e => {
      console.warn("dismissing alerts:", e);
      void loadAlerts();
    });
    if (mesh.length) invoke("notes_dismiss", { ids: mesh }).catch(e => {
      console.warn("dismissing notifications:", e);
      void loadNotes();
    });
  };

  /// A row's click: an alert opens in the pane, the rest open what they are about.
  const openItem = (i: Item) => {
    const to = opens(i);
    read([i.id], to === "alert");

    if (to === "alert" || to === "skill") {
      opened = i.id;
      draw();
      return;
    }

    if (!to) return;
    if (i.kind === "run") dismiss([i.id]);
    toggle(false);
    if (to === "project") go("projects", (i.target as { projects: ProjectsArg }).projects);
    else if (to === "session") go("pane", i.target as PaneTarget);
    else if (to === "sync") go("sync");
    else openSettings("machines");
  };

  const answer = (i: Item, allow: boolean) => {
    answering.add(i.id);
    failed.delete(i.id);
    draw();
    invoke("notes_answer", { id: i.id, allow })
      .catch(e => failed.set(i.id, String(e)))
      .finally(() => { answering.delete(i.id); draw(); });
  };

  /// A reply turns the alert into a chat: it leaves the pane and the chat opens with the reply sent.
  const reply = (i: Item, text: string) => {
    const message = text.trim();
    if (!message) return;
    if (alerts) alerts = without(alerts, [i.id]);
    toggle(false);
    go("pane", { chat: { session: i.id } });
    pane.ask(i.id, message).catch(e => console.warn("replying to an alert:", e));
  };

  /// Asks the machine holding `held` to keep it; the row waits until the conflict is told again.
  const keepCopy = (i: Item, held: Held) => {
    keeping.set(i.id, { hash: held.hash, was: told(i) });
    failed.delete(i.id);
    draw();
    invoke("skill_keep", { machine: held.machine, name: skillName(i), hash: held.hash }).catch(e => {
      keeping.delete(i.id);
      failed.set(i.id, reason(e));
      draw();
    });
  };

  show = session => {
    const already = open;
    open = true;
    opened = session && find(alerts ?? [], session) ? session : null;
    wanted = opened ? null : session;
    draw();
    if (opened) read([opened]);
    else if (!already) read(unreadIds(items()), false);
  };

  const loadAlerts = async () => {
    try {
      setAlerts(await invoke<Alerts>("alerts_list"));
    } catch (e) {
      console.warn("reading alerts:", e);
    }
  };

  const loadNotes = async () => {
    try {
      setNotes(await invoke<Note[]>("notes_list"));
    } catch (e) {
      console.warn("reading notifications:", e);
    }
  };

  document.addEventListener("click", e => { if (open && !e.composedPath().includes(host)) toggle(false); });
  document.addEventListener("keydown", e => {
    if (!e.defaultPrevented && commandKey(e) && !(WINDOWS ? e.metaKey : e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "b" &&
        !(e.target instanceof Element && e.target.closest("[contenteditable]:not([contenteditable=false]), .tm-xterm"))) {
      e.preventDefault();
      toggle(!open, true);
      return;
    }

    if (!open || e.key !== "Escape") return;
    e.preventDefault();
    // Esc goes back to the list first, then closes the pane.
    if (opened) backToList(); else toggle(false, true);
  });
  listen<Alerts>("alerts", e => setAlerts(e.payload));
  listen<Note[]>("notes", e => setNotes(e.payload));
  for (const name of ["link-state", "reconnected"]) listen(name, () => void loadAlerts());
  draw();
  void loadAlerts();
  void loadNotes();
}
