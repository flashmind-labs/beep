// The bell at the right end of every command center header, beside the usage
// pill, and the notifications pane it opens: what the assistant told the user
// on its own (a mail digest, an arrival, a review request), and what the mesh
// told this Mac (a coding agent finished or asks, sync conflicts, a failed
// move, a machine that went offline). Alerts are notifications, not chats,
// until the user replies in one.
import { pane } from "../bar/pane.ts";
import { el as h } from "../ui/dom.ts";
import { ago } from "../ui/format.ts";
import { dbtn as button, svgEl } from "../ui/parts.ts";
import { IC } from "../ui/icons.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { openSettings } from "../settings/sheet.ts";
import { type Alerts, type Item, type Note, asks, badge, bellLabel, bySide, doneLabel, find, fromAlert, fromNote, markRead, merge, opens, subLine, unreadCount, unreadIds, without } from "./notices-state.ts";
import { go } from "./views.ts";

const BELL = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 21h4"/></svg>';

/// Opens the pane, on alert `session` when one is named. Set by `mountNotices`.
let show: (session: string | null) => void = () => {};

/// A notification's click: the pane with that alert opened.
export function showNotices(session: string | null) {
  show(session);
}

/// Draws the bell into `host` and keeps it current.
export function mountNotices(host: HTMLElement) {
  /// The server's alerts, `null` with no server, and the mesh's notes.
  let alerts: Item[] | null = null;
  let notes: Item[] = [];
  let open = false;
  let opened: string | null = null;
  /// An alert a click asked for before the list had come.
  let wanted: string | null = null;
  /// Asks being answered, and why an answer failed, by id.
  const answering = new Set<string>();
  const failed = new Map<string, string>();
  host.classList.add("nt");

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
    if (opened && !find(alerts ?? [], opened)) opened = null;
    if (opened) read([opened]);
    draw();
  };

  const setNotes = (next: Note[]) => {
    notes = next.map(fromNote);
    // The list is redrawn unless an alert is open, with a reply maybe typed in it.
    if (open && opened) drawBell(); else draw();
  };

  const toggle = (to: boolean) => {
    open = to;
    if (!to) opened = null;
    draw();
    // Opening the pane reads what it shows; the rows keep their dots until it closes.
    if (to) read(unreadIds(items()), false);
  };

  const makeBell = () => {
    const all = items();
    const count = badge(all);
    const bell = h("button", "nt-bell", svgEl(BELL)!, count ? h("span", "nt-n", count) : null);
    bell.type = "button";
    bell.title = "Notifications";
    bell.setAttribute("aria-label", bellLabel(all));
    bell.setAttribute("aria-expanded", String(open));
    bell.addEventListener("click", () => toggle(!open));
    return bell;
  };

  /// Redraws the bell alone, leaving the pane as it is.
  const drawBell = () => {
    const first = host.firstElementChild;
    if (first) first.replaceWith(makeBell());
    else draw();
  };

  const draw = () => {
    const bell = makeBell();
    const shown = opened && find(alerts ?? [], opened);
    host.replaceChildren(bell, ...(open ? [shown ? openedView(shown) : listView(items())] : []));
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
    if (to === "alert") {
      opened = i.id;
      draw();
      return;
    }
    if (!to) return;
    toggle(false);
    if (to === "session") go("pane", i.target);
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

  const listView = (list: Item[]) => {
    const unread = unreadCount(list);
    const head = h("div", "nt-h", h("b", "", "Notifications"), unread ? h("span", "", `${unread} unread`) : null,
      list.length ? button("Clear", "small quiet", () => dismiss(list.map(i => i.id))) : null);
    const body = list.length
      ? list.map(row)
      : [h("div", "nt-empty", "No notifications")];
    return h("div", "nt-pop", head, h("div", "nt-list", ...body));
  };

  const row = (i: Item) => {
    const close = button(svgEl(IC.x)!, "quiet", e => { e.stopPropagation(); dismiss([i.id]); });
    close.setAttribute("aria-label", "Dismiss");
    close.title = "Dismiss";
    const over = doneLabel(i);
    const sub = h("span", "", subLine(i, ago(i.at)), over ? h("i", "nt-done", over) : null);
    const why = failed.get(i.id);
    const acts = asks(i)
      ? h("span", "nt-acts",
        button("Allow", "pri small", e => { e.stopPropagation(); answer(i, true); }),
        button("Deny", "small", e => { e.stopPropagation(); answer(i, false); }),
        why ? h("span", "nt-err", why) : null)
      : null;
    if (acts && answering.has(i.id)) for (const b of acts.querySelectorAll("button")) b.disabled = true;
    // A turn's text or the conflicts' paths, under the title.
    const text = h("span", "nt-t", h("b", "", i.title), !i.server && i.text ? h("span", "", i.text.split("\n").join(", ")) : null, sub, acts);
    const r = h("div", `nt-row${i.read ? "" : " unread"}${i.done ? " done" : ""}`, h("span", "nt-dot"), text, close);
    r.tabIndex = 0;
    r.setAttribute("role", "button");
    r.addEventListener("click", () => openItem(i));
    r.addEventListener("keydown", e => {
      if (e.target === r && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); openItem(i); }
    });
    return r;
  };

  const openedView = (i: Item) => {
    const back = button("Notifications", "small quiet", () => { opened = null; draw(); });
    const head = h("div", "nt-h", back, h("span", "", subLine(i, ago(i.at))), button("Dismiss", "small", () => dismiss([i.id])));
    const input = h("input", "nt-ask");
    input.type = "text";
    input.placeholder = "Reply to turn this into a chat";
    input.setAttribute("aria-label", "Reply");
    input.autocomplete = "off";
    input.spellcheck = false;
    input.addEventListener("keydown", e => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); reply(i, input.value); } });
    const send = button("Send", "pri small", () => reply(i, input.value));
    // The text is the assistant's own, shown as text and never as markup.
    const body = h("div", "nt-list nt-note", h("b", "", i.title), h("p", "", i.text));
    const pop = h("div", "nt-pop", head, body, h("div", "nt-rep", input, send));
    queueMicrotask(() => input.focus());
    return pop;
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
    if (!open || e.key !== "Escape") return;
    // Esc goes back to the list first, then closes the pane.
    if (opened) { opened = null; draw(); } else toggle(false);
  });
  listen<Alerts>("alerts", e => setAlerts(e.payload));
  listen<Note[]>("notes", e => setNotes(e.payload));
  for (const name of ["link-state", "reconnected"]) listen(name, () => void loadAlerts());
  draw();
  void loadAlerts();
  void loadNotes();
}
